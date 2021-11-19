mod attrs;

use attrs::{BuildableAttributes, FieldAttributes, VariantAttributes};
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use std::str::FromStr;
use syn::{parse_macro_input, spanned::Spanned, DataEnum, DataStruct, DeriveInput, Fields, Ident};

pub(crate) fn variant_to_kebab_case(ident: &str) -> String {
    let mut result = String::new();
    for (i, ch) in ident.chars().enumerate() {
        let lowercase = ch.to_ascii_lowercase();
        if i > 0 && ch != lowercase {
            result.push('-');
        }
        result.push(lowercase);
    }
    result
}

pub(crate) fn field_to_kebab_case(ident: &str) -> String {
    ident.replace('_', "-")
}

fn derive_fields(
    name: impl ToTokens,
    fields: &Fields,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let fields_data = match fields {
        Fields::Named(fields) => &fields.named,
        Fields::Unnamed(fields) => &fields.unnamed,
        Fields::Unit => return Ok(quote! {Ok((#name, input))}),
    };

    let mut field_definitions = Vec::new();
    let mut parse_required = Vec::new();
    let mut field_construct = Vec::new();
    let mut parse_optional = Vec::new();
    let mut required_count = 0;

    for (index, field) in fields_data.iter().enumerate() {
        let ident = format_ident!("field_{}", index);

        let field_type = &field.ty;
        field_definitions.push(quote! {
            let mut #ident: Option<#field_type> = None;
        });

        let attr = FieldAttributes::from_attributes(field.attrs.iter())?;
        let parse_expr = attr
            .parse_with
            .as_deref()
            .map(str::to_string)
            .map(|parse_with| {
                let fn_path = proc_macro2::TokenStream::from_str(&parse_with).unwrap();
                quote! {#fn_path(input)}
            })
            .unwrap_or_else(|| {
                quote! { <#field_type as ::cmd_parser::CmdParsable>::parse_cmd(input) }
            });

        if attr.is_required() {
            parse_required.push(quote! {
                #required_count => {
                    let (value, remaining) = #parse_expr?;
                    input = remaining;
                    #ident = Some(value);
                }
            });
            required_count += 1;
        } else {
            for (label, value) in &attr.attr_names {
                let label = format!("--{}", field_to_kebab_case(label));
                if let Some(value) = value {
                    let value = proc_macro2::TokenStream::from_str(value).unwrap();
                    parse_optional.push(quote! {
                        #label => { #ident = Some(#value) }
                    });
                } else {
                    parse_optional.push(quote! {
                        #label => {
                            let (value, remaining) = #parse_expr?;
                            input = remaining;
                            #ident = Some(value);
                        }
                    });
                }
            }
        }

        let unwrap = if attr.is_required() {
            quote! {.unwrap()}
        } else {
            quote! {.unwrap_or_default()}
        };
        if let Some(field_ident) = field.ident.as_ref() {
            field_construct.push(quote! {#field_ident: #ident #unwrap});
        } else {
            field_construct.push(quote! {#ident #unwrap});
        }
    }

    let construct = if let Fields::Named(_) = fields {
        quote! { Ok((#name { #(#field_construct),* }, input)) }
    } else {
        quote! { Ok((#name(#(#field_construct),*), input)) }
    };

    Ok(quote! {
        #(#field_definitions)*
        let mut index = 0;
        #[allow(unreachable_code)]
        loop {
            if input.starts_with("--") {
                let (token, remaining) = ::cmd_parser::take_token(input);
                input = remaining;
                let token = token.unwrap();
                match token.as_ref(){
                    #(#parse_optional)*
                    _ => {
                        return Err(::cmd_parser::ParseError{
                            kind: ::cmd_parser::ParseErrorKind::UnknownAttribute(token),
                            expected: "".into()
                        })
                    }
                }
            } else if index >= #required_count {
                break;
            } else {
                match index {
                    #(#parse_required)*
                    _ => unreachable!(),
                }
                index += 1;
            }
        }
        #construct
    })
}

fn derive_enum(name: Ident, data: DataEnum) -> Result<proc_macro2::TokenStream, syn::Error> {
    let mut variant_parse = Vec::new();
    let mut transparent_parse = Vec::new();
    for variant in data.variants.iter() {
        let variant_ident = &variant.ident;
        let variant_path = quote! { #name::#variant_ident };
        let parse_fields = derive_fields(variant_path, &variant.fields)?;

        let attrs = VariantAttributes::from_attributes(variant.attrs.iter())?;
        if attrs.transparent {
            transparent_parse.push(quote! {
                let parsed: ::std::result::Result<(#name ,&str), ::cmd_parser::ParseError> = (||{ #parse_fields })();
                if let Ok((result, remaining)) = parsed{
                    return Ok((result, remaining));
                }
            });
        } else {
            let mut discriminators = attrs.aliases;
            if !attrs.ignore {
                let label = variant_to_kebab_case(&variant.ident.to_string());
                discriminators.push(label);
            }
            if discriminators.is_empty() {
                continue;
            }

            let pattern = discriminators.iter().enumerate().map(|(index, value)| {
                if index == 0 {
                    quote! { #value }
                } else {
                    quote! { | #value }
                }
            });
            variant_parse.push(quote! {
                #(#pattern)* => { #parse_fields }
            });
        }
    }
    Ok(quote! {
        impl cmd_parser::CmdParsable for #name {
            fn parse_cmd_raw(mut original_input: &str) -> Result<(Self, &str), cmd_parser::ParseError<'_>> {
                let (discriminator, mut input) = cmd_parser::take_token(original_input);
                let discriminator = match discriminator {
                    Some(discriminator) => discriminator,
                    None => return Err(cmd_parser::ParseError {
                        kind: cmd_parser::ParseErrorKind::TokenRequired,
                        expected: "name".into(),
                    }),
                };

                let d_str: &str = &discriminator;
                match d_str {
                    #(#variant_parse)*
                    _ => {
                        let mut input = original_input;
                        #(#transparent_parse)*
                        Err(cmd_parser::ParseError{
                            kind: cmd_parser::ParseErrorKind::UnknownVariant(discriminator),
                            expected: "name".into(),
                        })
                    }
                }
            }
        }
    })
}

fn derive_struct(name: Ident, data: DataStruct) -> Result<proc_macro2::TokenStream, syn::Error> {
    let struct_create = derive_fields(&name, &data.fields)?;

    Ok(quote! {
        impl cmd_parser::CmdParsable for #name {
            fn parse_cmd_raw(mut input: &str) -> Result<(Self, &str), cmd_parser::ParseError<'_>> {
                #struct_create
            }
        }
    })
}

#[proc_macro_derive(CmdParsable, attributes(cmd))]
pub fn derive_parseable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    let result = match input.data {
        syn::Data::Struct(data) => derive_struct(name, data),
        syn::Data::Enum(data) => derive_enum(name, data),
        syn::Data::Union(data) => Err(syn::Error::new(
            data.union_token.span(),
            "parsing unions is not supported",
        )),
    };
    match result {
        Ok(token_stream) => token_stream.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{field_to_kebab_case, variant_to_kebab_case};

    #[test]
    fn rename_variant() {
        assert_eq!(&variant_to_kebab_case("Word"), "word");
        assert_eq!(&variant_to_kebab_case("TwoWords"), "two-words");
    }

    #[test]
    fn rename_field() {
        assert_eq!(&field_to_kebab_case("word"), "word");
        assert_eq!(&field_to_kebab_case("two_words"), "two-words");
    }
}
