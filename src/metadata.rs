use html5ever::local_name;
use kuchiki::NodeRef;


const TITLE_CANDIDATES: [&str; 6] = [
    "og:title", "twitter:title", "dc:title", "dcterm:title",
    "weibo:article:title", "weibo:webpage:title", 
];


pub struct Metadata {
    pub page_title: Option<String>,
    pub article_title: Option<String>,
    // pub byline: Option<String>,
    // pub description: Option<String>,
}


pub fn extract(root: &NodeRef) -> Metadata {
    let mut page_title = root.select_first("title")
        .map(|node| Some(node.text_contents()))
        .unwrap_or(None);

    let mut article_title = find_article_title(root);

    // if let (None, Some(at)) = (&page_title, &article_title) {
    //     page_title = Some(at.clone());
    // }
    // if let(Some(pt), None) = (page_title, article_title) {
    //     article_title = Some(pt.clone());
    // }

    match (&page_title, &article_title) {
        (None, Some(at)) => {page_title = Some(at.clone());},
        (Some(pt), None) => {article_title = Some(pt.clone());},
        _ => (),
    }

    Metadata {page_title, article_title}
}


fn find_article_title(root: &NodeRef) -> Option<String> {
    let meta_type_attrs = [
        local_name!("name"),
        local_name!("property"),
        local_name!("itemprop"),
    ];
    // look for meta tags with `name`, `property`, or `itemprop` 
    for meta in root.select("meta").unwrap() {
        for attr in meta_type_attrs.iter() {
            if let Some(type_name) = meta.attributes.borrow().get(attr) {
                if TITLE_CANDIDATES.contains(&type_name) {
                    if let Some(content) = meta.attributes.borrow().get(local_name!("content")) {
                        return Some(content.to_string());
                    }
                }
            }
        }
    }

    // if no qualifying meta tag is found, look for h1s
    // only use an h1 as title if there are no others in the document
    let mut h1s = root.select("h1").unwrap();
    match (h1s.next(), h1s.next()) {
        (Some(h), None) => return Some(h.text_contents()),
        // we don't want to accept an h2 below if there are multiple h1s
        (Some(_), Some(_)) => return None,
        _ => (),
    }

    // same deal for h2's
    let mut h2s = root.select("h2").unwrap();
    match (h2s.next(), h2s.next()) {
        (Some(h), None) => return Some(h.text_contents()),
        _ => (),
    }
    None
}


use kuchiki::{parse_html, traits::TendrilSink};

#[test]
fn test_extract() {
    const DOC: &str = 
        "<!doctype html>
        <head>
            <title>Some Article - Some Site</title>
            <meta name=\"og:title\" content=\"Some Article\">
        </head>
        <body>
        </body>";

    let root = kuchiki::parse_html().one(DOC);
    let metadata = extract(&root);
    assert_eq!(metadata.page_title, Some("Some Article - Some Site".into()));
    assert_eq!(metadata.article_title, Some("Some Article".into()));
}
