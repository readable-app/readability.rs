use std::io::Write;

use kuchikiki::traits::TendrilSink;
use kuchikiki::NodeData::*;
use kuchikiki::NodeRef;
use serde::Deserialize;
use url::Url;

use readable_readability::{Metadata, Readability};

// duplicate the Metadata struct so we can implement Deserialize
#[derive(Deserialize)]
struct TestMetadata {
    pub page_title: Option<String>,
    pub article_title: Option<String>,
    pub image_url: Option<String>,
    pub byline: Option<String>,
    pub description: Option<String>,
}

fn compare_metadata(actual: &Metadata, expected: &TestMetadata) {
    assert_eq!(actual.page_title, expected.page_title);
    assert_eq!(actual.article_title, expected.article_title);
    assert_eq!(actual.image_url, expected.image_url);
    assert_eq!(actual.byline, expected.byline);
    assert_eq!(actual.description, expected.description);
}

fn compare_trees(actual: &NodeRef, expected: &NodeRef) {
    compare_nodes(actual, expected);

    let mut actual_it = actual.children().filter(is_not_empty_text);
    let mut expected_it = expected.children().filter(is_not_empty_text);

    loop {
        let actual = actual_it.next();
        let expected = expected_it.next();

        match (actual, expected) {
            (None, None) => break,
            (None, Some(node)) => panic!("Expected {}", stringify_node(&node)),
            (Some(node), None) => panic!("Needless {}", stringify_node(&node)),
            (Some(one), Some(two)) => compare_trees(&one, &two),
        }
    }
}

fn is_not_empty_text(node: &NodeRef) -> bool {
    !node
        .as_text()
        .map_or(false, |text| text.borrow().trim().is_empty())
}

fn compare_nodes(actual: &NodeRef, expected: &NodeRef) {
    let actual_data = actual.data();
    let expected_data = expected.data();

    match (actual_data, expected_data) {
        (&Element(ref actual_data), &Element(ref expected_data)) => {
            let actual_attributes = &actual_data.attributes.borrow().map;
            let expected_attributes = &expected_data.attributes.borrow().map;

            if actual_data.name != expected_data.name || actual_attributes != expected_attributes {
                panic!(
                    "{} != {}",
                    stringify_node(&actual),
                    stringify_node(&expected)
                );
            }
        }

        (&Text(ref actual), &Text(ref expected)) => {
            let actual = actual.borrow();
            let expected = expected.borrow();

            let actual_words = actual.split_whitespace();
            let expected_words = expected.split_whitespace();

            if actual_words.ne(expected_words) {
                panic!("TEXT: {} != {}", *actual, *expected);
            }
        }

        (&Comment(_), &Comment(_))
        | (&Doctype(_), &Doctype(_))
        | (&Document(_), &Document(_))
        | (&DocumentFragment, &DocumentFragment) => unimplemented!(),

        _ => panic!("{} != {}", stringify_node(actual), stringify_node(expected)),
    };
}

fn stringify_node(node: &NodeRef) -> String {
    const LIMIT: usize = 40;

    let string = node.to_string();

    match *node.data() {
        Element(_) => {
            let mut pos = 0;

            for slice in string.split_terminator('>') {
                pos += slice.len() + 1;
                if pos >= LIMIT {
                    break;
                }
            }

            string[..pos].to_owned()
        }
        _ if string.len() > LIMIT => format!("{}...", &string[..LIMIT]),
        _ => string,
    }
}

fn setup_logger() {
    let _ = env_logger::Builder::new()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .parse_default_env()
        .try_init();
}

macro_rules! include_sample_file {
    ($name:ident, $file:expr) => {
        include_str!(concat!("../samples/", stringify!($name), "/", $file))
    };
}

macro_rules! test_sample {
    ($name:ident) => {
        #[test]
        fn $name() {
            static SOURCE: &'static str = include_sample_file!($name, "source.html");
            static EXPECTED: &'static str = include_sample_file!($name, "expected.html");
            static EXPECTED_META: &'static str = include_sample_file!($name, "metadata.json");

            setup_logger();

            let (actual_tree, actual_meta) = Readability::new()
                .base_url(Url::parse("http://fakehost/test/page.html").unwrap())
                .parse(SOURCE);

            let expected_tree = kuchikiki::parse_html()
                .one(EXPECTED)
                .select("body > *")
                .unwrap()
                .next()
                .unwrap()
                .as_node()
                .clone();

            compare_trees(&actual_tree, &expected_tree);

            let expected_meta = serde_json::from_str(EXPECTED_META).unwrap();
            compare_metadata(&actual_meta, &expected_meta);
        }
    };
}

test_sample!(base_url);
test_sample!(social_buttons);
test_sample!(replace_font_tags);

test_sample!(bbc);
test_sample!(buzzfeed);
test_sample!(cnet);
test_sample!(ehow_2);
test_sample!(heise);
test_sample!(herald_sun);
test_sample!(iab);
test_sample!(libertation);
test_sample!(medium_1);
test_sample!(medium_2);
test_sample!(mozilla_1);
test_sample!(msn);
test_sample!(nytimes_1);
test_sample!(wikia);
test_sample!(wikipedia);
test_sample!(wordpress);
