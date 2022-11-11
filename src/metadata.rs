use html5ever::local_name;
use kuchiki::NodeRef;


const TITLE_KEYS: [&str; 6] = [
    "og:title", "twitter:title", "dc:title", "dcterm:title",
    "weibo:article:title", "weibo:webpage:title",
];
const BYLINE_KEYS: [&str; 6] = [
    "author", "dc:creator", "dcterm:creator", "og:article:author",
    "article:author", "byl",
];
const DESCRIPTION_KEYS: [&str; 7] = [
    "description", "dc:description", "dcterm:description", "og:description",
    "weibo:article:description", "weibo:webpage:description", "twitter:description"
];


pub struct Metadata {
    pub page_title: Option<String>,
    pub article_title: Option<String>,
    pub byline: Option<String>,
    pub description: Option<String>,
}


pub fn extract(root: &NodeRef) -> Metadata {
    let mut page_title = root.select_first("title")
        .map(|node| node.text_contents())
        .ok();

    let mut article_title = get_article_title(root);

    match (&page_title, &article_title) {
        (None, Some(at)) => {page_title = Some(at.clone());},
        (Some(pt), None) => {article_title = Some(pt.clone());},
        _ => (),
    }

    let byline = extract_meta_content(root, &BYLINE_KEYS);
    let description = get_article_description(root);
    Metadata {page_title, article_title, byline, description}
}


fn get_article_title(root: &NodeRef) -> Option<String> {
    let meta_title = extract_meta_content(root, &TITLE_KEYS);
    if meta_title.is_some() {
        return meta_title;
    }

    // if no qualifying meta tag is found, look for a single h1
    // if there are multiple h1s, give up
    let mut h1s = root.select("h1").unwrap();
    match (h1s.next(), h1s.next()) {
        (Some(h), None) => return Some(h.text_contents()),
        // we don't want to accept an h2 below if there are multiple h1s
        (Some(_), Some(_)) => return None,
        _ => (),
    }

    // same deal for h2's
    let mut h2s = root.select("h2").unwrap();
    if let (Some(h), None) = (h2s.next(), h2s.next()) {
        return Some(h.text_contents())
    }
    None
}


fn get_article_description(root: &NodeRef) -> Option<String> {
    let meta_desc = extract_meta_content(root, &DESCRIPTION_KEYS);
    if meta_desc.is_some() {
        return meta_desc;
    }

    // if the description isn't specified in a meta tag, use the text of the first <p>
    root.select_first("p")
        .map(|p| p.text_contents())
        .ok()
}


// Given a root node and a list of meta keys, return the content of the first meta tag
// with its `name`, `property`, or `itemprop` attribute set to one of the expected types.
fn extract_meta_content(root: &NodeRef, expected_types: &[&str]) -> Option<String> {
    let meta_type_attrs = [
        local_name!("name"),
        local_name!("property"),
        local_name!("itemprop"),
    ];
    // unwrap is safe here because select() only errors when you give it an invalid CSS selector
    for meta_node in root.select("meta").unwrap() {
        for attr_name in &meta_type_attrs {
            let attributes = meta_node.attributes.borrow();
            if let Some(meta_type) = attributes.get(attr_name) {
                if expected_types.contains(&meta_type) {
                    if let Some(content) = attributes.get(local_name!("content")) {
                        return Some(content.to_string());
                    }
                }
            }
        }
    }
    None
}


#[allow(unused_imports)]
use kuchiki::{parse_html, traits::TendrilSink};

#[test]
fn test_extract() {
    const DOC: &str = 
        "<!doctype html>
        <head>
            <title>Some Article - Some Site</title>
            <meta name=\"og:title\" content=\"Some Article\">
            <meta property=\"author\" content=\"Joe Schmoe\">
            <meta itemprop=\"dcterm:description\" content=\"A test article for test cases.\">
        </head>
        <body>
        </body>";

    let root = kuchiki::parse_html().one(DOC);
    let metadata = extract(&root);
    assert_eq!(metadata.page_title, Some("Some Article - Some Site".into()));
    assert_eq!(metadata.article_title, Some("Some Article".into()));
}
