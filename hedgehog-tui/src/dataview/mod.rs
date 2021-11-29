pub(crate) mod interactive;
pub(crate) mod linear;
pub(crate) mod paginated;
mod version;

use cmd_parser::CmdParsable;
use hedgehog_library::model::Identifiable;
use std::ops::Range;
pub(crate) use version::Versioned;

#[derive(Debug, Clone)]
pub(crate) struct DataViewOptions {
    page_size: usize,
    load_margins: usize,
    scroll_margins: usize,
}

impl Default for DataViewOptions {
    fn default() -> Self {
        DataViewOptions {
            page_size: 128,
            load_margins: 32,
            scroll_margins: 3,
        }
    }
}

pub(crate) trait DataView {
    type Item;
    type Request;
    type Message;

    fn init(request_data: impl Fn(Self::Request), options: DataViewOptions) -> Self;
    fn item_at(&self, index: usize) -> Option<&Self::Item>;
    fn size(&self) -> Option<usize>;
    fn update(&mut self, range: Range<usize>, request_data: impl Fn(Self::Request));
    fn handle(&mut self, msg: Self::Message) -> bool;
    fn has_data(&self) -> bool;
    fn index_of<ID: Eq>(&self, id: ID) -> Option<usize>
    where
        Self::Item: Identifiable<Id = ID>;
}

pub(crate) trait EditableDataView {
    type Id;
    type Item: Identifiable<Id = Self::Id>;

    fn remove(&mut self, id: Self::Id) -> Option<usize>;
    fn add(&mut self, item: Self::Item);
}

pub(crate) trait UpdatableDataView {
    type Id;
    type Item: Identifiable<Id = Self::Id>;

    fn update(&mut self, id: Self::Id, callback: impl FnOnce(&mut Self::Item));
    fn update_all(&mut self, callback: impl Fn(&mut Self::Item));
    fn update_at(&mut self, index: usize, callback: impl FnOnce(&mut Self::Item));
}

fn index_with_id<'a, T: Identifiable + 'a>(
    items: impl Iterator<Item = &'a T>,
    id: T::Id,
) -> Option<usize> {
    items
        .enumerate()
        .filter(|(_, item)| item.id() == id)
        .map(|(index, _)| index)
        .next()
}

pub(crate) trait DataProvider {
    type Request;

    fn request(&self, request: Versioned<Self::Request>);
}

#[derive(Debug, Clone, Copy, PartialEq, CmdParsable)]
pub(crate) enum CursorCommand {
    Next,
    Previous,
    PageUp,
    PageDown,
    First,
    Last,
}

fn request_data<P: DataProvider>(provider: &Versioned<Option<P>>, message: P::Request) {
    let message = provider.with_data(message);
    provider.as_ref().map(|provider| {
        if let Some(provider) = provider {
            provider.request(message);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        interactive::InteractiveList, linear::ListData, paginated::PaginatedData,
        paginated::PaginatedDataMessage, paginated::PaginatedDataRequest, DataProvider, DataView,
        DataViewOptions, Versioned,
    };
    use hedgehog_library::model::Identifiable;
    use hedgehog_library::Page;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    const TEST_OPTIONS: DataViewOptions = DataViewOptions {
        page_size: 4,
        load_margins: 1,
        scroll_margins: 1,
    };

    #[derive(Debug)]
    struct MockDataProvider<T> {
        requests: Rc<RefCell<VecDeque<Versioned<T>>>>,
    }

    impl<T> MockDataProvider<T> {
        fn new() -> (Self, Rc<RefCell<VecDeque<Versioned<T>>>>) {
            let requests = Rc::new(RefCell::new(VecDeque::new()));
            let provider = MockDataProvider {
                requests: requests.clone(),
            };
            (provider, requests)
        }
    }

    impl<T> DataProvider for MockDataProvider<T> {
        type Request = T;

        fn request(&self, request: Versioned<Self::Request>) {
            self.requests.borrow_mut().push_back(request);
        }
    }

    fn assert_list<P: DataProvider, T: PartialEq + std::fmt::Debug + Clone + Identifiable>(
        list: &InteractiveList<impl DataView<Item = T, Request = P::Request>, P>,
        expected: &[(Option<T>, bool)],
    ) {
        assert_eq!(
            list.iter()
                .unwrap()
                .map(|(a, b)| (a.cloned(), b))
                .collect::<Vec<(Option<T>, bool)>>()
                .as_slice(),
            expected,
        );
    }

    #[derive(Debug, Clone, Eq, PartialEq)]
    struct SimpleItem<T>(T);

    impl<T: Clone + Eq> Identifiable for SimpleItem<T> {
        type Id = T;

        fn id(&self) -> Self::Id {
            self.0.clone()
        }
    }

    macro_rules! item {
        ($value:expr) => {
            (Some($value), false)
        };
        ($value:expr, sel) => {
            (Some($value), true)
        };
    }

    macro_rules! s_item {
        ($value:expr) => {
            (Some(SimpleItem($value)), false)
        };
        ($value:expr, sel) => {
            (Some(SimpleItem($value)), true)
        };
    }

    macro_rules! s_items {
        ($($val:expr),*) => {
            vec![$(SimpleItem($val)),*]
        };
        ($val:expr; $size:expr) => {
            vec![SimpleItem($val); $size]
        };
    }

    macro_rules! no_item {
        () => {
            (None, false)
        };
        (selected) => {
            (None, true)
        };
    }

    #[test]
    fn scrolling_list_data() {
        let mut scroll_list =
            InteractiveList::<ListData<SimpleItem<u8>>, MockDataProvider<_>>::new_with_options(
                4,
                TEST_OPTIONS,
            );
        assert!(scroll_list.iter().is_none());

        let (provider, requests) = MockDataProvider::new();
        scroll_list.set_provider(provider);
        assert!(scroll_list.iter().is_none());
        let request = requests.borrow_mut().pop_front().unwrap();
        assert!(requests.borrow().is_empty());
        requests.borrow_mut().clear();

        scroll_list.handle_data(request.with_data(s_items![1, 2, 3, 4, 5, 6]));
        let expected_forward = vec![
            [s_item!(1, sel), s_item!(2), s_item!(3), s_item!(4)],
            [s_item!(1), s_item!(2, sel), s_item!(3), s_item!(4)],
            [s_item!(1), s_item!(2), s_item!(3, sel), s_item!(4)],
            [s_item!(2), s_item!(3), s_item!(4, sel), s_item!(5)],
            [s_item!(3), s_item!(4), s_item!(5, sel), s_item!(6)],
            [s_item!(3), s_item!(4), s_item!(5), s_item!(6, sel)],
            [s_item!(3), s_item!(4), s_item!(5), s_item!(6, sel)],
        ];
        for expected in expected_forward {
            assert_list(&scroll_list, &expected);
            scroll_list.move_cursor(1);
        }
        assert_eq!(scroll_list.selection(), Some(&SimpleItem(6)));

        let expected_backward = vec![
            [s_item!(3), s_item!(4), s_item!(5), s_item!(6, sel)],
            [s_item!(3), s_item!(4), s_item!(5, sel), s_item!(6)],
            [s_item!(3), s_item!(4, sel), s_item!(5), s_item!(6)],
            [s_item!(2), s_item!(3, sel), s_item!(4), s_item!(5)],
            [s_item!(1), s_item!(2, sel), s_item!(3), s_item!(4)],
            [s_item!(1, sel), s_item!(2), s_item!(3), s_item!(4)],
            [s_item!(1, sel), s_item!(2), s_item!(3), s_item!(4)],
        ];
        for expected in expected_backward {
            assert_list(&scroll_list, &expected);
            scroll_list.move_cursor(-1);
        }
        assert_eq!(scroll_list.selection(), Some(&SimpleItem(1)));
    }

    #[test]
    fn scrolling_fits_on_screen() {
        let mut scroll_list =
            InteractiveList::<ListData<SimpleItem<u8>>, MockDataProvider<_>>::new_with_options(
                4,
                TEST_OPTIONS,
            );
        assert!(scroll_list.iter().is_none());

        let (provider, requests) = MockDataProvider::new();
        scroll_list.set_provider(provider);
        assert!(scroll_list.iter().is_none());
        let request = requests.borrow_mut().pop_front().unwrap();
        assert!(requests.borrow().is_empty());
        requests.borrow_mut().clear();

        scroll_list.handle_data(request.with_data(s_items![1, 2, 3]));
        let expected_both_ways = vec![
            [s_item!(1, sel), s_item!(2), s_item!(3)],
            [s_item!(1, sel), s_item!(2), s_item!(3)],
            [s_item!(1), s_item!(2, sel), s_item!(3)],
            [s_item!(1), s_item!(2), s_item!(3, sel)],
            [s_item!(1), s_item!(2), s_item!(3, sel)],
        ];
        for expected in expected_both_ways.iter().skip(1) {
            assert_list(&scroll_list, expected);
            scroll_list.move_cursor(1);
        }
        for expected in expected_both_ways.iter().rev().skip(1) {
            assert_list(&scroll_list, expected);
            scroll_list.move_cursor(-1);
        }
    }

    #[test]
    fn initializing_paginated_list() {
        let mut scroll_list =
            InteractiveList::<PaginatedData<SimpleItem<u8>>, MockDataProvider<_>>::new_with_options(
                6,
                TEST_OPTIONS,
            );
        assert!(scroll_list.iter().is_none());

        let (provider, requests) = MockDataProvider::new();
        scroll_list.set_provider(provider);
        assert!(scroll_list.iter().is_none());

        let request = requests.borrow_mut().pop_front().unwrap();
        assert_eq!(request.as_inner(), &PaginatedDataRequest::Size);
        scroll_list.handle_data(request.with_data(PaginatedDataMessage::Size(20)));

        assert_list(
            &scroll_list,
            &[
                no_item!(selected),
                no_item!(),
                no_item!(),
                no_item!(),
                no_item!(),
                no_item!(),
            ],
        );

        assert_eq!(
            requests.borrow_mut().pop_front().unwrap().into_inner(),
            PaginatedDataRequest::Page(Page::new(0, 4))
        );
        assert_eq!(
            requests.borrow_mut().pop_front().unwrap().into_inner(),
            PaginatedDataRequest::Page(Page::new(1, 4))
        );

        scroll_list.handle_data(request.with_data(PaginatedDataMessage::Page {
            index: 1,
            values: s_items![4, 5, 6, 7],
        }));
        assert_list(
            &scroll_list,
            &[
                no_item!(selected),
                no_item!(),
                no_item!(),
                no_item!(),
                s_item!(4),
                s_item!(5),
            ],
        );

        scroll_list.handle_data(request.with_data(PaginatedDataMessage::Page {
            index: 0,
            values: s_items![0, 1, 2, 3],
        }));
        assert_list(
            &scroll_list,
            &[
                s_item!(0, sel),
                s_item!(1),
                s_item!(2),
                s_item!(3),
                s_item!(4),
                s_item!(5),
            ],
        );

        scroll_list.move_cursor(6);
        assert_list(
            &scroll_list,
            &[
                s_item!(2),
                s_item!(3),
                s_item!(4),
                s_item!(5),
                s_item!(6, sel),
                s_item!(7),
            ],
        );
        assert_eq!(
            requests.borrow_mut().pop_front().unwrap().into_inner(),
            PaginatedDataRequest::Page(Page::new(2, 4))
        );

        scroll_list.move_cursor(1);
        assert!(requests.borrow().is_empty());
        assert_list(
            &scroll_list,
            &[
                s_item!(3),
                s_item!(4),
                s_item!(5),
                s_item!(6),
                s_item!(7, sel),
                no_item!(),
            ],
        );
    }

    #[test]
    fn creating_and_dropping_pages() {
        let mut options = TEST_OPTIONS.clone();
        options.page_size = 3;
        let mut scroll_list =
            InteractiveList::<PaginatedData<SimpleItem<u8>>, MockDataProvider<_>>::new_with_options(
                4, options,
            );

        let (provider, requests) = MockDataProvider::new();
        scroll_list.set_provider(provider);
        assert!(scroll_list.iter().is_none());

        let request = requests.borrow_mut().pop_front().unwrap();
        assert_eq!(request.as_inner(), &PaginatedDataRequest::Size);
        scroll_list.handle_data(request.with_data(PaginatedDataMessage::Size(100)));

        let scrolling_data = vec![
            (0, 2, [s_item!(0, sel), s_item!(0), s_item!(0), s_item!(1)]),
            (0, 2, [s_item!(0), s_item!(0, sel), s_item!(0), s_item!(1)]),
            (0, 2, [s_item!(0), s_item!(0), s_item!(0, sel), s_item!(1)]),
            (0, 2, [s_item!(0), s_item!(0), s_item!(1, sel), s_item!(1)]),
            (0, 3, [s_item!(0), s_item!(1), s_item!(1, sel), s_item!(1)]),
            (0, 3, [s_item!(1), s_item!(1), s_item!(1, sel), s_item!(2)]),
            (1, 2, [s_item!(1), s_item!(1), s_item!(2, sel), s_item!(2)]),
            (1, 3, [s_item!(1), s_item!(2), s_item!(2, sel), s_item!(2)]),
            (1, 3, [s_item!(2), s_item!(2), s_item!(2, sel), s_item!(3)]),
            (2, 2, [s_item!(2), s_item!(2), s_item!(3, sel), s_item!(3)]),
        ];
        for (page_index, offset, expected) in scrolling_data {
            while let Some(request) = requests.borrow_mut().pop_front() {
                match request.as_inner() {
                    PaginatedDataRequest::Size => panic!(),
                    PaginatedDataRequest::Page(page) => {
                        scroll_list.handle_data(request.with_data(PaginatedDataMessage::Page {
                            index: page.index,
                            values: s_items![page.index as u8; page.size],
                        }));
                    }
                }
            }
            assert_list(&scroll_list, &expected);
            let (actual_index, actual_offset) = scroll_list.data.pages_range();
            assert_eq!(page_index, actual_index);
            assert_eq!(offset, actual_offset);

            scroll_list.move_cursor(1);
        }

        let scrolling_backwards_data = vec![
            (2, 3, [s_item!(2), s_item!(3), s_item!(3, sel), s_item!(3)]),
            (2, 3, [s_item!(2), s_item!(3, sel), s_item!(3), s_item!(3)]),
            (2, 2, [s_item!(2), s_item!(2, sel), s_item!(3), s_item!(3)]),
            (1, 3, [s_item!(2), s_item!(2, sel), s_item!(2), s_item!(3)]),
            (1, 3, [s_item!(1), s_item!(2, sel), s_item!(2), s_item!(2)]),
            (1, 2, [s_item!(1), s_item!(1, sel), s_item!(2), s_item!(2)]),
            (0, 3, [s_item!(1), s_item!(1, sel), s_item!(1), s_item!(2)]),
            (0, 3, [s_item!(0), s_item!(1, sel), s_item!(1), s_item!(1)]),
            (0, 2, [s_item!(0), s_item!(0, sel), s_item!(1), s_item!(1)]),
            (0, 2, [s_item!(0), s_item!(0, sel), s_item!(0), s_item!(1)]),
            (0, 2, [s_item!(0, sel), s_item!(0), s_item!(0), s_item!(1)]),
        ];
        for (page_index, offset, expected) in scrolling_backwards_data {
            while let Some(request) = requests.borrow_mut().pop_front() {
                match request.as_inner() {
                    PaginatedDataRequest::Size => panic!(),
                    PaginatedDataRequest::Page(page) => {
                        scroll_list.handle_data(request.with_data(PaginatedDataMessage::Page {
                            index: page.index,
                            values: s_items![page.index as u8; page.size],
                        }));
                    }
                }
            }
            assert_list(&scroll_list, &expected);
            let (actual_index, actual_offset) = scroll_list.data.pages_range();
            assert_eq!(page_index, actual_index);
            assert_eq!(offset, actual_offset);

            scroll_list.move_cursor(-1);
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct IdItem(usize, &'static str);

    impl Identifiable for IdItem {
        type Id = usize;

        fn id(&self) -> Self::Id {
            self.0
        }
    }

    #[test]
    fn editing_data() {
        let mut scroll_list =
            InteractiveList::<ListData<IdItem>, MockDataProvider<_>>::new_with_options(
                10,
                TEST_OPTIONS,
            );

        let (provider, requests) = MockDataProvider::new();
        scroll_list.set_provider(provider);
        let request = requests.borrow_mut().pop_front().unwrap();
        scroll_list.handle_data(request.with_data(vec![
            IdItem(5, "five"),
            IdItem(3, "three"),
            IdItem(7, "seven"),
            IdItem(1, "one"),
        ]));

        scroll_list.add_item(IdItem(8, "eight"));
        assert_list(
            &scroll_list,
            &[
                item!(IdItem(5, "five"), sel),
                item!(IdItem(3, "three")),
                item!(IdItem(7, "seven")),
                item!(IdItem(1, "one")),
                item!(IdItem(8, "eight")),
            ],
        );

        scroll_list.replace_item(IdItem(3, "three v2"));
        assert_list(
            &scroll_list,
            &[
                item!(IdItem(5, "five"), sel),
                item!(IdItem(3, "three v2")),
                item!(IdItem(7, "seven")),
                item!(IdItem(1, "one")),
                item!(IdItem(8, "eight")),
            ],
        );

        scroll_list.remove_item(7);
        assert_list(
            &scroll_list,
            &[
                item!(IdItem(5, "five"), sel),
                item!(IdItem(3, "three v2")),
                item!(IdItem(1, "one")),
                item!(IdItem(8, "eight")),
            ],
        );
    }

    #[test]
    fn deleting_items() {
        let mut scroll_list =
            InteractiveList::<ListData<IdItem>, MockDataProvider<_>>::new_with_options(
                10,
                TEST_OPTIONS,
            );

        let (provider, requests) = MockDataProvider::new();
        scroll_list.set_provider(provider);
        let request = requests.borrow_mut().pop_front().unwrap();
        scroll_list.handle_data(request.with_data(vec![
            IdItem(1, "a"),
            IdItem(2, "b"),
            IdItem(3, "c"),
            IdItem(4, "d"),
            IdItem(5, "e"),
            IdItem(6, "f"),
            IdItem(7, "g"),
        ]));
        scroll_list.move_cursor(3);

        assert_list(
            &scroll_list,
            &[
                item!(IdItem(1, "a")),
                item!(IdItem(2, "b")),
                item!(IdItem(3, "c")),
                item!(IdItem(4, "d"), sel),
                item!(IdItem(5, "e")),
                item!(IdItem(6, "f")),
                item!(IdItem(7, "g")),
            ],
        );

        scroll_list.remove_item(6);
        assert_list(
            &scroll_list,
            &[
                item!(IdItem(1, "a")),
                item!(IdItem(2, "b")),
                item!(IdItem(3, "c")),
                item!(IdItem(4, "d"), sel),
                item!(IdItem(5, "e")),
                item!(IdItem(7, "g")),
            ],
        );

        scroll_list.remove_item(3);
        assert_list(
            &scroll_list,
            &[
                item!(IdItem(1, "a")),
                item!(IdItem(2, "b")),
                item!(IdItem(4, "d"), sel),
                item!(IdItem(5, "e")),
                item!(IdItem(7, "g")),
            ],
        );

        scroll_list.remove_item(4);
        assert_list(
            &scroll_list,
            &[
                item!(IdItem(1, "a")),
                item!(IdItem(2, "b")),
                item!(IdItem(5, "e"), sel),
                item!(IdItem(7, "g")),
            ],
        );

        scroll_list.remove_item(7);
        assert_list(
            &scroll_list,
            &[
                item!(IdItem(1, "a")),
                item!(IdItem(2, "b")),
                item!(IdItem(5, "e"), sel),
            ],
        );

        scroll_list.remove_item(5);
        assert_list(
            &scroll_list,
            &[item!(IdItem(1, "a")), item!(IdItem(2, "b"), sel)],
        );

        scroll_list.remove_item(2);
        assert_list(&scroll_list, &[item!(IdItem(1, "a"), sel)]);

        scroll_list.remove_item(1);
        assert_list(&scroll_list, &[]);

        scroll_list.add_item(IdItem(8, "h"));
        assert_list(&scroll_list, &[item!(IdItem(8, "h"), sel)]);
    }
}
