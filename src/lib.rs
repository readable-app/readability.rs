use std::cmp;
use std::f32;
use std::fmt;
use std::iter;

use html5ever::{local_name, namespace_url, ns, QualName};
use kuchikiki::iter::NodeIterator;
use kuchikiki::traits::TendrilSink;
use kuchikiki::{Attributes, ElementData, NodeData, NodeDataRef, NodeRef};
use lazy_static::lazy_static;
use log::trace;
use regex::Regex;
use url::Url;

pub use metadata::Metadata;
use node_cache::NodeCache;

mod metadata;
mod node_cache;

// TODO: add examples.
// TODO: document it!

type ElemRef = NodeDataRef<ElementData>;

trait NodeRefExt {
    fn node_ref(&self) -> &NodeRef;

    fn is(&self, name: QualName) -> bool {
        self.node_ref()
            .as_element()
            .map_or(false, |e| e.name == name)
    }

    fn replace<N: NodeRefExt>(&self, node: &N) {
        self.node_ref().insert_before(node.node_ref().clone());
        self.remove();
    }

    fn remove(&self) {
        self.node_ref().detach();
    }

    fn rename(&self, name: QualName) -> NodeRef {
        let node = self.node_ref();

        if let Some(elem) = node.as_element() {
            // I'd like find a way to do this without clone(), but
            // I'm not sure how because BTreeMap doesn't have drain()
            let attributes = elem.attributes.borrow();
            let replacement = NodeRef::new_element(name, attributes.map.clone());

            for child in node.children() {
                replacement.append(child);
            }

            node.replace(&replacement);

            replacement
        } else {
            node.clone()
        }
    }

    fn previous_element(&self) -> Option<ElemRef> {
        self.node_ref().preceding_siblings().elements().next()
    }
}

impl NodeRefExt for NodeRef {
    #[inline]
    fn node_ref(&self) -> &NodeRef {
        self
    }
}

impl NodeRefExt for ElemRef {
    #[inline]
    fn node_ref(&self) -> &NodeRef {
        self.as_node()
    }

    fn is(&self, name: QualName) -> bool {
        self.name == name
    }
}

lazy_static! {
    static ref UNLIKELY_CANDIDATE: Regex = Regex::new(r"(?xi)
        -ad-|ai2html|banner|breadcrumbs|combx|comment|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|
        modal|related|remark|replies|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|
        ad-break|agegate|pagination|pager|popup|yom-remote
    ").unwrap();

    static ref MAYBE_CANDIDATE: Regex = Regex::new(r"(?xi)
        and|article|body|column|main|shadow
    ").unwrap();

    static ref POSITIVE: Regex = Regex::new(r"(?xi)
        article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story
    ").unwrap();

    static ref NEGATIVE: Regex = Regex::new(r"(?xi)
        -ad-|hidden|^hid$|\shid$|\shid\s|^hid\s|banner|combx|comment|com-|contact|foot|footer|footnote|
        gdpr|masthead|media|meta|modal|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|
        sponsor|shopping|tags|tool|widget
    ").unwrap();

    // TODO: restore byline parsing.
    //static ref BYLINE: Regex = Regex::new(r"(?xi)
        //byline|author|dateline|writtenby|p-author
    //").unwrap();

    static ref VIDEO: Regex = Regex::new(r"(?xi)
        //(www\.)?(dailymotion|youtube|youtube-nocookie|player\.vimeo)\.com
    ").unwrap();

    static ref PROTOCOL: Regex = Regex::new(r"^\w+:").unwrap();
}

macro_rules! tag {
    ($name:tt) => {
        QualName {
            prefix: None,
            ns: ns!(html),
            local: local_name!($name),
        }
    };
}

macro_rules! attrib {
    ($name:tt) => {
        local_name!($name)
    };
}

fn extract_byline(elem: &ElemRef) -> Option<String> {
    let attributes = elem.attributes.borrow();

    let rel = attributes.get(attrib!("rel")).unwrap_or("");
    //let classes = attributes.get(attrib!("class")).unwrap_or("");
    //let id = attributes.get(attrib!("id")).unwrap_or("");

    // TODO: uncomment it after traverse repearing.
    //let is_byline = rel == "author" || BYLINE.is_match(classes) || BYLINE.is_match(id);
    let is_byline = rel == "author";

    if is_byline {
        // TODO: traverse subtrees manually to preserve spaces?
        let text = elem.text_contents();
        let byline = text.trim();

        #[allow(clippy::len_zero)]
        if 0 < byline.len() && byline.len() < 100 {
            Some(byline.to_string())
        } else {
            None
        }
    } else {
        None
    }
}

fn is_unlikely_candidate(elem: &ElemRef) -> bool {
    match elem.name {
        tag!("a") | tag!("body") => return false,
        _ => {}
    }

    let attributes = elem.attributes.borrow();

    // Ok, check classes.
    let classes = attributes.get(attrib!("class")).unwrap_or("");
    let id = attributes.get(attrib!("id")).unwrap_or("");

    (UNLIKELY_CANDIDATE.is_match(classes) || UNLIKELY_CANDIDATE.is_match(id))
        && !(MAYBE_CANDIDATE.is_match(classes) || MAYBE_CANDIDATE.is_match(id))
}

fn transform_div(div: &ElemRef) {
    debug_assert_eq!(div.name, tag!("div"));

    let node = div.as_node();

    if has_single_p(node) {
        trace!("    => replacing <{}> with inner <p>", format_tag(node));
        let p = node.children().elements().next().unwrap();
        node.replace(&p);
    } else if !has_block_elem(node) {
        trace!("    => altering <{}> to <p>", format_tag(node));
        node.rename(tag!("p"));
    } else {
        // TODO: move to upper level.
        for child in node.children() {
            if let Some(text) = child.as_text() {
                trace!("    => moving text node in <{}> with <p>", format_tag(node));
                let text = text.borrow();

                if text.trim().is_empty() {
                    continue;
                }

                let p = NodeRef::new_element(tag!("p"), iter::empty());
                child.replace(&p);
                p.append(child.clone());
            }
        }
    }
}

fn has_single_p(node: &NodeRef) -> bool {
    let mut child_it = node.children().elements();
    let child = child_it.next();

    if child.is_none() || child_it.next().is_some() {
        return false;
    }

    let child = child.unwrap();
    if !child.is(tag!("p")) {
        return false;
    }

    node.children()
        .text_nodes()
        .all(|t| t.borrow().trim().is_empty())
}

fn has_block_elem(node: &NodeRef) -> bool {
    node.descendants().elements().any(|elem| {
        matches! {
            elem.name,
            tag!("a") | tag!("blockquote") | tag!("dl") | tag!("div") | tag!("img") | tag!("ol") |
            tag!("p") | tag!("pre") | tag!("table") | tag!("ul") | tag!("select")
        }
    })
}

fn count_chars(text: &str) -> (u32, u32) {
    let mut char_cnt = 0;
    let mut comma_cnt = 0;

    // TODO: what about graphemes?
    let mut iter = text.trim().chars().peekable();

    while let Some(ch) = iter.next() {
        if ch.is_whitespace() {
            if !iter.peek().unwrap().is_whitespace() {
                char_cnt += 1;
            }
        } else if ch == ',' {
            if iter.peek().map_or(true, |&c| c != ',') {
                char_cnt += 1;
                comma_cnt += 1;
            }
        } else {
            char_cnt += 1;
        }
    }

    (char_cnt, comma_cnt)
}

fn is_tag_to_score(tag: &QualName) -> bool {
    matches! {
        *tag,
        tag!("section") | tag!("p") | tag!("td") | tag!("pre") |
        tag!("h2") | tag!("h3") | tag!("h4") | tag!("h5") | tag!("h6")
    }
}

fn tag_score(tag: &QualName) -> f32 {
    match *tag {
        tag!("section") => 15.,
        tag!("div") => 5.,
        tag!("pre") | tag!("td") | tag!("blockquote") => 3.,
        tag!("address") | tag!("form") => -3.,
        tag!("dl") | tag!("dt") | tag!("dd") => -3.,
        tag!("li") | tag!("ol") | tag!("ul") => -3.,
        tag!("body") => -5.,
        tag!("h1") | tag!("h2") | tag!("h3") | tag!("h4") | tag!("h5") | tag!("h6") => -5.,
        tag!("th") => -5.,
        _ => 0.,
    }
}

fn class_score(elem: &ElemRef) -> f32 {
    let attributes = elem.attributes.borrow();
    let mut score = 0.;

    if let Some(classes) = attributes.get(attrib!("class")) {
        if POSITIVE.is_match(classes) {
            score += 25.;
        }
        if NEGATIVE.is_match(classes) {
            score -= 25.;
        }
    }

    if let Some(id) = attributes.get(attrib!("id")) {
        if POSITIVE.is_match(id) {
            score += 25.;
        }
        if NEGATIVE.is_match(id) {
            score -= 25.;
        }
    }

    score
}

fn is_stuffed(elem: &ElemRef, info: &NodeInfo) -> bool {
    match elem.name {
        // TODO: remove <object>, <embed> etc.
        tag!("h1") | tag!("footer") | tag!("button") => false,

        tag!("div")
        | tag!("section")
        | tag!("header")
        | tag!("h2")
        | tag!("h3")
        | tag!("h4")
        | tag!("h5")
        | tag!("h6") => {
            if info.text_len == 0 {
                let children_count = elem.as_node().children().count() as u32;

                if children_count == 0 || children_count == info.br_count + info.hr_count {
                    return false;
                }
            }

            true
        }

        tag!("thead") | tag!("tbody") | tag!("th") | tag!("tr") | tag!("td") =>
        // TODO: add <video> and <audio> counters to the sum.
        {
            info.text_len > 0 || info.img_count + info.embed_count + info.iframe_count > 0
        }

        tag!("p") | tag!("pre") | tag!("blockquote") =>
        // TODO: add <video> and <audio> counters to the sum.
        {
            info.img_count + info.embed_count + info.iframe_count > 0 ||
            // TODO: calculate length without construction the string.
                !elem.text_contents().trim().is_empty()
        }

        _ => true,
    }
}

fn clean_attributes(attributes: &mut Attributes) {
    // TODO: what about removing all except for `alt`, `href`, `src` and `title`?
    attributes.remove(attrib!("style"));
}

fn fix_relative_urls(attributes: &mut Attributes, base_url: &Url) {
    fn fix(url: &mut String, base: &Url) {
        // Ignore absolute and hash urls.
        if url.is_empty() || PROTOCOL.is_match(url) || url.starts_with('#') {
            return;
        }

        if let Ok(resolved) = base.join(url) {
            *url = resolved.into();
        }
    }

    if let Some(attr) = attributes.get_mut(attrib!("href")) {
        fix(attr, base_url);
    }

    if let Some(attr) = attributes.get_mut(attrib!("src")) {
        fix(attr, base_url);
    }
}

fn is_acceptable_top_level(tag: &QualName) -> bool {
    matches!(
        *tag,
        tag!("div") | tag!("article") | tag!("section") | tag!("p")
    )
}

#[derive(Default, PartialEq, Clone)]
struct NodeInfo {
    content_score: f32,
    text_len: u32,
    link_len: u32,
    commas: u32,
    is_candidate: bool,
    is_shabby: bool,

    p_count: u32,
    img_count: u32,
    li_count: u32,
    input_count: u32,
    embed_count: u32,
    iframe_count: u32,
    br_count: u32,
    hr_count: u32,
}

impl fmt::Debug for NodeInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut s = fmt.debug_struct("");

        if self.content_score > 0. {
            s.field("content_score", &self.content_score);
        }
        if self.text_len > 0 {
            s.field("text", &self.text_len);
        }
        if self.link_len > 0 {
            s.field("link", &self.link_len);
        }
        if self.commas > 0 {
            s.field("commas", &self.commas);
        }
        if self.is_candidate {
            s.field("candidate", &self.is_candidate);
        }
        if self.is_shabby {
            s.field("shabby", &self.is_shabby);
        }
        if self.p_count > 0 {
            s.field("p", &self.p_count);
        }
        if self.img_count > 0 {
            s.field("img", &self.img_count);
        }
        if self.li_count > 0 {
            s.field("li", &self.li_count);
        }
        if self.input_count > 0 {
            s.field("input", &self.input_count);
        }
        if self.embed_count > 0 {
            s.field("embed", &self.embed_count);
        }
        if self.iframe_count > 0 {
            s.field("iframe", &self.iframe_count);
        }
        if self.br_count > 0 {
            s.field("br", &self.br_count);
        }
        if self.hr_count > 0 {
            s.field("hr", &self.hr_count);
        }

        s.finish()
    }
}

pub struct Readability {
    info: NodeCache<NodeInfo>,
    candidates: Vec<ElemRef>,
    byline: Option<String>,

    strip_unlikelys: bool,
    weight_classes: bool,
    clean_conditionally: bool,
    clean_attributes: bool,
    base_url: Option<Url>,
}

impl Default for Readability {
    fn default() -> Self {
        Self::new()
    }
}

impl Readability {
    pub fn new() -> Readability {
        Readability {
            info: NodeCache::new(),
            candidates: Vec::new(),
            byline: None,

            strip_unlikelys: true,
            weight_classes: true,
            clean_conditionally: true,
            clean_attributes: true,
            base_url: None,
        }
    }

    pub fn strip_unlikelys(&mut self, enabled: bool) -> &mut Self {
        self.strip_unlikelys = enabled;
        self
    }

    pub fn weight_classes(&mut self, enabled: bool) -> &mut Self {
        self.weight_classes = enabled;
        self
    }

    pub fn clean_conditionally(&mut self, enabled: bool) -> &mut Self {
        self.clean_conditionally = enabled;
        self
    }

    pub fn clean_attributes(&mut self, enabled: bool) -> &mut Self {
        self.clean_attributes = enabled;
        self
    }

    pub fn base_url<U>(&mut self, url: U) -> &mut Self
    where
        U: Into<Option<Url>>,
    {
        self.base_url = url.into();
        self
    }

    pub fn parse(&mut self, html: &str) -> (NodeRef, Metadata) {
        let top_level = kuchikiki::parse_html().one(html);

        let metadata = metadata::extract(&top_level);

        let top_level = top_level
            .select("html > body")
            .unwrap()
            .next()
            .map_or(top_level, |b| b.as_node().clone());

        top_level.detach();

        // TODO: retry with fewer restrictions.
        (self.readify(top_level), metadata)
    }

    fn readify(&mut self, top_level: NodeRef) -> NodeRef {
        let mut current = top_level.clone();
        let mut bubbling = false;

        // TODO: refactor this shitty traverse!
        // TODO: ignore or remove empty text nodes.
        loop {
            if !bubbling {
                self.on_capturing(&current);

                if let Some(child) = current.first_child() {
                    current = child;
                    continue;
                }

                bubbling = true;
            }

            let (ncurrent, nbubbling) = if let Some(next) = current.next_sibling() {
                (next, false)
            } else if let Some(parent) = current.parent() {
                (parent, bubbling)
            } else {
                if bubbling {
                    self.on_bubbling(&current);
                }
                break;
            };

            if bubbling {
                self.on_bubbling(&current);
            }

            bubbling = nbubbling;
            current = ncurrent;
        }

        if self.candidates.is_empty() {
            return top_level;
        }

        self.score_candidates();

        if self.candidates.is_empty() {
            return top_level;
        }

        let top_candidate = self.find_common_candidate();
        self.correct_candidate(top_candidate)

        // TODO: combine top candidates together.
    }

    // Capturing stage: remove unlikely candidates, unpack divs etc.
    fn on_capturing(&mut self, node: &NodeRef) {
        if node.as_element().is_some() {
            trace!("<{}>", format_tag(node));
        }

        for child in node.children() {
            let remove = match *child.data() {
                NodeData::Comment(_) | NodeData::DocumentFragment => true,
                NodeData::Text(ref data) => data.borrow().trim().is_empty(),
                NodeData::Element(ref elem) => {
                    matches!(elem.name, tag!("script") | tag!("style") | tag!("noscript"))
                }
                _ => false,
            };

            if remove {
                if child.as_element().is_some() {
                    trace!(
                        "    => removing <{}> as useless element",
                        format_tag(&child)
                    );
                }

                child.remove();
            }

            if let Some(child) = child.into_element_ref() {
                // TODO: mozilla/readability takes into account only first occurrence.
                //if self.byline.is_none() {
                if let Some(byline) = extract_byline(&child) {
                    self.byline = Some(byline);
                    trace!(
                        "    => removing <{}> as byline container",
                        format_tag(&child)
                    );
                    child.remove();

                    continue;
                }
                //}

                if self.strip_unlikelys && is_unlikely_candidate(&child) {
                    trace!(
                        "    => removing <{}> as unlikely candidate",
                        format_tag(&child)
                    );
                    child.remove();
                } else if child.is(tag!("div")) {
                    transform_div(&child);
                } else if child.is(tag!("font")) {
                    trace!("    => altering <{}> to <div>", format_tag(&child));
                    child.rename(tag!("span"));
                }
            }
        }
    }

    // Bubbling stage: collect info based on children and score elements.
    fn on_bubbling(&mut self, node: &NodeRef) {
        match *node.data() {
            NodeData::Text(ref data) => {
                let (char_cnt, comma_cnt) = count_chars(&data.borrow()[..]);

                let parent = node.parent().unwrap();

                let parent_info = self.info.get_or_create(&parent);
                parent_info.text_len += char_cnt;
                parent_info.commas += comma_cnt;
            }
            NodeData::Element(ElementData {
                ref name,
                ref attributes,
                ..
            }) => {
                trace!(
                    "</{}> {}",
                    format_tag(node),
                    format_info(self.info.get(node))
                );

                // FIXME: don't propagate info of bad nodes.
                self.propagate_info(node);

                if is_tag_to_score(name) {
                    self.score_node(node);
                }

                let elem = node.clone().into_element_ref().unwrap();

                // TODO: don't create info if it's not necessary.
                if !is_stuffed(&elem, self.info.get_or_create(node)) {
                    node.remove();
                    trace!("    => removed (it's not stuffed)");

                    return;
                }

                if self.clean_conditionally && !self.is_conditionally_acceptable(&elem) {
                    if let Some(info) = self.info.get(node) {
                        info.is_candidate = false;
                    }

                    if let Some(info) = node.parent().map(|parent| self.info.get_or_create(&parent))
                    {
                        info.is_shabby = true;
                    }

                    node.remove();
                    trace!("    => removed (it's conditionally unacceptable)");

                    return;
                }

                // Remove <br> preceding <p>.
                if node.is(tag!("p")) {
                    if let Some(elem) = node.previous_element() {
                        if elem.is(tag!("br")) {
                            elem.remove();
                        }
                    }
                }

                let mut attributes = attributes.borrow_mut();

                if self.clean_attributes {
                    clean_attributes(&mut attributes);
                }

                if let Some(ref base_url) = self.base_url {
                    if *name == tag!("a") || *name == tag!("img") {
                        fix_relative_urls(&mut attributes, base_url);
                    }
                }
            }
            _ => {}
        };
    }

    fn propagate_info(&mut self, node: &NodeRef) {
        let parent = match node.parent() {
            Some(parent) => parent,
            None => return,
        };

        let is_a = node.is(tag!("a"));
        // TODO: avoid extra cloning.
        let info = self.info.get_or_create(node).clone();

        let parent_info = self.info.get_or_create(&parent);

        if let Some(elem) = node.as_element() {
            match elem.name {
                tag!("p") => parent_info.p_count += 1,
                tag!("img") => parent_info.img_count += 1,
                tag!("li") => parent_info.li_count += 1,
                tag!("input") => parent_info.input_count += 1,
                tag!("br") => parent_info.br_count += 1,
                tag!("hr") => parent_info.hr_count += 1,
                tag!("iframe") => parent_info.iframe_count += 1,
                tag!("embed") => {
                    let attribs = elem.attributes.borrow();
                    let src = attribs.get(attrib!("src")).unwrap_or("");

                    if !VIDEO.is_match(src) {
                        parent_info.embed_count += 1;
                    }
                }
                _ => {}
            };
        }

        parent_info.link_len += if is_a { info.text_len } else { info.link_len };
        parent_info.text_len += info.text_len;
        parent_info.commas += info.commas;
        parent_info.p_count += info.p_count;
        parent_info.img_count += info.img_count;
        parent_info.li_count += info.li_count;
        parent_info.input_count += info.input_count;
        parent_info.embed_count += info.embed_count;
        parent_info.iframe_count += info.iframe_count;
        parent_info.br_count += info.br_count;
        parent_info.hr_count += info.hr_count;
    }

    fn is_conditionally_acceptable(&mut self, elem: &ElemRef) -> bool {
        let is_list = match elem.name {
            tag!("form") | tag!("fieldset") | tag!("table") | tag!("div") => false,
            tag!("ul") | tag!("ol") => true,
            _ => return true,
        };

        // TODO: cache the score to prevent extra calculations.
        let class_score = if self.weight_classes {
            class_score(elem)
        } else {
            0.
        };

        if class_score < 0. {
            return false;
        }

        let info = self.info.get_or_create(elem.as_node());

        if info.commas >= 10 {
            return true;
        }

        let link_density = info.link_len as f32 / info.text_len as f32;
        let p_img_ratio = info.p_count as f32 / info.img_count as f32;

        // TODO: take into account ancestor tags (check "figure").
        !((info.img_count > 1 && p_img_ratio < 0.5)
            || (!is_list && info.li_count > info.p_count + 100)
            || (info.input_count * 3 > info.p_count)
            || (!is_list && info.text_len < 25 && (info.img_count == 0 || info.img_count > 2))
            || (!is_list && class_score < 25. && link_density > 0.2)
            || (class_score >= 25. && link_density > 0.5)
            || ((info.embed_count == 1 && info.text_len < 75) || info.embed_count > 1))
    }

    fn score_node(&mut self, node: &NodeRef) {
        if let Some(content_score) = self.calculate_content_score(node) {
            self.propagate_score(node, content_score);
        }
    }

    fn calculate_content_score(&mut self, node: &NodeRef) -> Option<f32> {
        let parent_elem = node.parent().and_then(|p| p.into_element_ref());

        #[allow(clippy::question_mark)]
        if parent_elem.is_none() {
            return None;
        }

        let info = self.info.get_or_create(node);

        if info.text_len < 25 {
            return None;
        }

        // Add a point for the paragraph itself as a base.
        let mut content_score = 1;

        // Add points for any commas within this paragraph.
        content_score += info.commas;

        // For every 100 characters in this paragraph, add another point. Up to 3 points.
        let total_len = info.text_len + info.link_len;
        content_score += cmp::min(total_len / 100, 3);

        Some(content_score as f32)
    }

    fn propagate_score(&mut self, node: &NodeRef, content_score: f32) {
        for (level, ancestor) in node.ancestors().elements().enumerate().take(3) {
            let div = match level {
                0 => 1.,
                1 => 2.,
                _ => 3. * level as f32,
            };

            let addition = content_score / div;

            let info = self.info.get_or_create(ancestor.as_node());

            info.content_score += addition;

            if !info.is_candidate {
                self.candidates.push(ancestor);
                info.is_candidate = true;
            }
        }
    }

    fn score_candidates(&mut self) {
        trace!("Found {} candidates. Scoring...", self.candidates.len());

        let mut scored_candidates = Vec::with_capacity(self.candidates.len());

        for candidate in self.candidates.drain(..) {
            trace!(
                "Candidate: <{}> {}",
                format_tag(&candidate),
                format_info(self.info.get(candidate.as_node()))
            );

            let info = self.info.get(candidate.as_node()).unwrap();

            // The node has been removed.
            if !info.is_candidate {
                trace!("    => the node has been removed!");
                continue;
            }

            debug_assert!(info.text_len > 0);
            debug_assert!(info.text_len >= info.link_len);

            // Add content points.
            let mut score = info.content_score;

            // Add points for tag name.
            score += tag_score(&candidate.name);

            // Add points for an class/id weight.
            if self.weight_classes {
                score += class_score(&candidate);
            }

            // Scale the final score based on link density. Good content should have a relatively
            // small link density (5% or less) and be mostly unaffected by this operation.
            score *= 1. - info.link_len as f32 / info.text_len as f32;

            trace!("    => score: {}", score);

            debug_assert!(score.is_finite());

            scored_candidates.push((score, candidate));
        }

        if scored_candidates.is_empty() {
            trace!("There are no actual candidates!");
            return;
        }

        scored_candidates.sort_by(|&(a, _), &(b, _)| b.partial_cmp(&a).unwrap());

        let score_threshold = scored_candidates[0].0 * 0.75;

        let top_candidate_it = scored_candidates
            .into_iter()
            .take_while(|&(score, _)| score >= score_threshold)
            .map(|(_, candidate)| candidate);

        self.candidates.extend(top_candidate_it);

        trace!("After scoring left {} candidates", self.candidates.len());
    }

    fn find_common_candidate(&self) -> NodeRef {
        // TODO: mozilla/readability uses 3 here, but we still have problems.
        const MIN_CANDIDATES: usize = 4;

        trace!("Searching for common parent...");

        let best = self.candidates[0].as_node();

        if self.candidates.len() < MIN_CANDIDATES
            || best.is(tag!("body"))
            || best.parent().map_or(true, |p| p.is(tag!("body")))
        {
            return best.clone();
        }

        for common in best.ancestors().take_while(|n| !n.is(tag!("body"))) {
            let mut n = 0;

            for candidate in &self.candidates[1..] {
                if candidate.as_node().ancestors().any(|a| a == common) {
                    n += 1;
                }

                if n == MIN_CANDIDATES {
                    trace!(
                        "Found common parent of top candidates: <{}>",
                        format_tag(&common)
                    );
                    return common;
                }
            }
        }

        best.clone()
    }

    fn correct_candidate(&mut self, mut candidate: NodeRef) -> NodeRef {
        trace!("Correcting candidate...");

        // Because of the bonus system, parents of candidates might have scores themselves.
        // if we see the score going *up* in the first few steps up the tree, that's a decent sign
        // that there might be more content lurking in other places that we want to unify in.
        let mut last_score = self.info.get_or_create(&candidate).content_score;
        let score_threshold = last_score / 3.;

        for parent in candidate.ancestors().take_while(|n| !n.is(tag!("body"))) {
            let parent_score = self.info.get_or_create(&parent).content_score;

            if parent_score < score_threshold {
                break;
            }

            if parent_score > last_score {
                candidate = parent;
                trace!("New candidate: <{}> (enough score)", format_tag(&candidate));
            }

            last_score = parent_score;
        }

        // If the top candidate is the only child, use parent instead. This will help sibling
        // joining logic when adjacent content is actually located in parent's sibling node.
        let parent_it = candidate.ancestors().take_while(|parent| {
            let mut child_it = parent.children();

            !parent.is(tag!("body"))
                && child_it.next().is_some()
                && child_it.next().is_none()
                && self.info.get(parent).map_or(true, |info| !info.is_shabby)
        });

        let result = parent_it.last().map_or(candidate, |parent| {
            trace!("New candidate: <{}> (single child)", format_tag(&parent));
            parent
        });

        if !is_acceptable_top_level(&result.as_element().unwrap().name) {
            trace!("Altering result: <{}> to <div>", format_tag(&result));
            result.rename(tag!("div"))
        } else {
            result
        }
    }
}

fn format_tag<N: NodeRefExt>(node: &N) -> String {
    let elem = node.node_ref().as_element().unwrap();
    let tag = &elem.name.local;
    let attributes = elem.attributes.borrow();

    match (attributes.get("id"), attributes.get("class")) {
        (Some(id), Some(class)) => format!("{} id=\"{}\" class=\"{}\"", tag, id, class),
        (Some(id), None) => format!("{} id=\"{}\"", tag, id),
        (None, Some(class)) => format!("{} class=\"{}\"", tag, class),
        (None, None) => format!("{}", tag),
    }
}

fn format_info(info: Option<&mut NodeInfo>) -> String {
    info.map_or_else(String::new, |i| format!("{:?}", i))
}
