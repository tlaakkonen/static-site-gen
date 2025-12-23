use std::{collections::VecDeque, io::Read, path::PathBuf};
use serde::{Serialize, Deserialize};
use pulldown_cmark as cmark;
use pulldown_latex as latex;
use crate::SiteBuilder;


#[derive(Debug, Serialize)]
pub struct Post {
    pub age: i64,
    pub id: String,
    pub source: String,
    pub meta: PostMeta
}

#[derive(Debug, Serialize)]
pub struct PostMeta {
    pub title: String,
    pub date: toml_datetime::Datetime,
    pub tags: Vec<String>,
    pub ghcomment: Option<(u32, Vec<String>)>
}

#[derive(Debug)]
pub struct PostBuilder<'a, 'b> {
    pub site: &'a mut SiteBuilder<'b>,
    pub file: PathBuf,
    pub dir: Option<PathBuf>,
    pub meta: Option<PostMeta>
}

impl<'a, 'b> PostBuilder<'a, 'b> {
    fn resolve_file(&self, path: &str) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let dpath = dir.join(path);
        dpath.is_file().then_some(dpath)
    }

    fn get_file_name(&self) -> String {
        if let Some(dir) = &self.dir {
            dir.file_name().and_then(|s| s.to_str())
                .unwrap_or("unnamed-post")
                .to_string()
        } else {
            self.file.file_name().and_then(|s| s.to_str())
                .unwrap_or("unnamed-post")
                .trim_end_matches(".md")
                .to_string()
        }
    }

    fn get_default_title(&self) -> String {
        println!("warning: post does not have a title, using file/directory name");
        self.get_file_name()
    }

    fn get_default_date(&self) -> toml_datetime::Datetime {
        use chrono::{Datelike, Timelike};
        println!("warning: post does not have a date, using the file creation time");
        let systime = self.file.metadata()
            .and_then(|m| m.created())
            .inspect_err(|e| println!("error: could not get file creation time: {e}"))
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let cdt = chrono::DateTime::<chrono::Local>::from(systime);
        let date = toml_datetime::Date { year: cdt.year() as u16, month: cdt.month() as u8, day: cdt.day() as u8 };
        let time = toml_datetime::Time { hour: cdt.hour() as u8, minute: cdt.minute() as u8, second: cdt.second() as u8, nanosecond: cdt.nanosecond() };
        let offset = if cdt.offset().local_minus_utc() == 0 {
            toml_datetime::Offset::Z
        } else {
            toml_datetime::Offset::Custom { minutes: (cdt.offset().local_minus_utc() / 60) as i16 }
        };
        toml_datetime::Datetime { date: Some(date), time: Some(time), offset: Some(offset) }
    }

    fn default_metadata(&self) -> PostMeta {
        let meta = PostMeta {
            title: self.get_default_title(),
            date: self.get_default_date(),
            tags: Vec::new(),
            ghcomment: None
        };
        println!(
            "warning: post does not have metadata, using defaults:\n    title = {:?},\n    date = {},\n    tags = {:?}\n    ghcomment = {:?}", 
            meta.title, meta.date, meta.tags, meta.ghcomment
        );
        meta
    }

    pub fn build(mut self) -> Option<Post> {
        println!("info: processing post `{}`", self.file.display());
        let Ok(contents) = std::fs::File::open(&self.file)
            .inspect_err(|e| println!("error: cannot read post: {e}")) 
            .and_then(|mut f| { let mut buf = String::new(); f.read_to_string(&mut buf)?; Ok(buf) })
            else { return None };
        
        let opts = cmark::Options::ENABLE_GFM 
            | cmark::Options::ENABLE_FOOTNOTES 
            | cmark::Options::ENABLE_STRIKETHROUGH
            | cmark::Options::ENABLE_SMART_PUNCTUATION
            | cmark::Options::ENABLE_MATH
            | cmark::Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS;
        let parser = cmark::Parser::new_ext(&contents, opts);
        let c_im_stream = CodeImageProcessor { 
            iter: cmark::TextMergeStream::new(parser), 
            post: &mut self,
            highlighter: arborium::Highlighter::new(), 
            buffer: VecDeque::new() 
        };
        let stream = MathProcessor { iter: c_im_stream, storage: latex::Storage::new() };
        let mut buffer = String::new();
        cmark::html::push_html(&mut buffer, stream);
        
        let id = self.get_file_name();
        let meta = if let Some(meta) = self.meta { meta } else { self.default_metadata() };
        let age = crate::dt_toml_to_chrono(&meta.date).signed_duration_since(&chrono::DateTime::UNIX_EPOCH).num_seconds();

        Some(Post {
            source: buffer,
            meta, id, age
        })
    }
}

#[derive(Debug, Deserialize)]
struct PostMetaIncomplete {
    title: Option<String>,
    date: Option<toml_datetime::Datetime>,
    tags: Option<Vec<String>>,
    ghcommentid: Option<u32>,
    ghcommentauthors: Option<Vec<String>>
}

const WRITE_OPTIONS: svgcleaner::WriteOptions = svgcleaner::WriteOptions {
    indent: svgdom::Indent::None,
    use_single_quote: false,
    attributes_indent: svgdom::Indent::None,
    trim_hex_colors: false,
    write_hidden_attributes: false,
    remove_leading_zero: false,
    use_compact_path_notation: false,
    join_arc_to_flags: false,
    remove_duplicated_path_commands: false,
    use_implicit_lineto_commands: false,
    simplify_transform_matrices: false,
    list_separator: svgdom::ListSeparator::Space,
    attributes_order: svgdom::AttributesOrder::AsIs
};
const CLEANING_OPTIONS: svgcleaner::CleaningOptions = svgcleaner::CleaningOptions {
    remove_unreferenced_ids: true,
    remove_default_attributes: true,
    remove_desc: true,
    remove_unused_defs: true,
    convert_shapes: false,
    remove_title: true,
    remove_metadata: true,
    remove_dupl_linear_gradients: true,
    remove_dupl_radial_gradients: true,
    remove_dupl_fe_gaussian_blur: true,
    ungroup_groups: true,
    ungroup_defs: true,
    group_by_style: true,
    merge_gradients: true,
    regroup_gradient_stops: false,
    remove_invalid_stops: false,
    remove_invisible_elements: true,
    resolve_use: true,
    remove_version: true,
    trim_ids: true,
    remove_text_attributes: true,
    remove_unused_coordinates: true,
    remove_xmlns_xlink_attribute: true,
    remove_needless_attributes: true,
    apply_transform_to_gradients: true,
    apply_transform_to_paths: true,
    apply_transform_to_shapes: true,
    remove_gradient_attributes: true,
    remove_unused_segments: true,
    coordinates_precision: 3,
    properties_precision: 3,
    transforms_precision: 3,
    paths_coordinates_precision: 3,
    paths_to_relative: false,
    convert_segments: false,
    join_style_attributes: svgcleaner::StyleJoinMode::Some
};

struct CodeImageProcessor<'a, 'b, 'c, I> {
    iter: I,
    post: &'b mut PostBuilder<'a, 'c>,
    highlighter: arborium::Highlighter,
    buffer: VecDeque<cmark::Event<'b>>
}

impl<'a, 'b, 'c, I: Iterator<Item=cmark::Event<'b>>> CodeImageProcessor<'a, 'b, 'c, I> {
    fn accumulate_plain_text(&mut self, tag: cmark::TagEnd, desc: &str) -> Option<String> {
        let mut text = String::new();
        loop {
            let Some(ev) = self.iter.next() else { return None; };
            self.buffer.push_back(ev.clone());

            match ev {
                cmark::Event::End(t) if t == tag => break,
                cmark::Event::InlineMath(m) => { text.push('$'); text.push_str(&m); text.push('$'); },
                cmark::Event::Text(t) => text.push_str(&t),
                _ => {
                    println!("error: could not parse {}, found {:?}", desc, ev);
                    return None
                }
            }
        }
        Some(text)
    }

    fn handle_svg_image(&mut self, path: PathBuf, alt: String, event: cmark::Event<'b>) -> Option<cmark::Event<'b>> {
        let mut source = String::new();
        if let Err(e) = std::fs::File::open(&path)
            .and_then(|mut f| f.read_to_string(&mut source)) {
            println!("error: could not read image file `{}`: {}", path.display(), e);
            return Some(event)
        }

        let cleaned = if let Ok(mut document) = svgcleaner::cleaner::parse_data(&source, &Default::default()) {
            if let None = svgcleaner::cleaner::clean_doc(&mut document, &CLEANING_OPTIONS, &WRITE_OPTIONS)
                .ok().and_then(|_| {
                    let mut svg = document.svg_element()?;
                    svg.set_attribute_checked(("role", "img")).ok()?;
                    let mut title = document.create_element(svgdom::ElementId::Title);
                    title.append(&document.create_node(svgdom::NodeType::Text, &alt));
                    svg.prepend(&title);
                    Some(())
                }) 
            {
                println!("warning: svg optimization failed for `{}`", path.display());
                source
            } else {
                let hash = {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::hash::DefaultHasher::new();
                    source.hash(&mut hasher);
                    (hasher.finish() & 0xffff) as u16
                };

                document.drain(|c| !matches!(c.node_type(), svgdom::NodeType::Element | svgdom::NodeType::Text));
                for (_, mut node) in document.descendants().svg() {
                    if node.has_id() {
                        node.set_id(format!("{:04x}-{}", hash, node.id()))
                    }
                }
                let mut cleaned = Vec::new();
                svgcleaner::cleaner::write_buffer(&document, &WRITE_OPTIONS, &mut cleaned);
                String::from_utf8_lossy(&cleaned).into()
            }
        } else {
            println!("warning: svg optimization failed for `{}`", path.display());
            source
        };

        println!("info: inlined svg image `{}`", path.display());
        self.buffer.pop_back();
        self.buffer.push_back(cmark::Event::Html("</figcaption></figure>".into()));
        self.buffer.push_front(cmark::Event::Html("<figcaption>".into()));
        self.buffer.push_front(cmark::Event::Html(cleaned.into()));
        Some(cmark::Event::Html("<figure>".into()))
    }

    fn handle_raster_image(&mut self, path: PathBuf, alt: String, event: cmark::Event<'b>) -> Option<cmark::Event<'b>> {
        let Ok(im) = image::open(&path)
            .inspect_err(|e| println!("error: could not read image file `{}`: {}", path.display(), e))
            else { return Some(event); };
        let mut buffer = Vec::new();
        let codec = image::codecs::webp::WebPEncoder::new_lossless(&mut buffer);
        println!("info: transcoding image file `{}`", path.display());
        let Ok(()) = im.write_with_encoder(codec)
            .inspect_err(|e| println!("error: could not reencode image file `{}`: {}", path.display(), e))
            else { return Some(event); };
        let url = format!("/{}", self.post.site.store_asset(buffer, "webp"));

        self.buffer.pop_back();
        self.buffer.push_back(cmark::Event::Html("</figcaption></figure>".into()));
        self.buffer.push_front(cmark::Event::Html("<figcaption>".into()));
        self.buffer.push_front(cmark::Event::Html(format!("<img src=\"{}\" alt=\"{}\">", url, alt).into()));
        Some(cmark::Event::Html("<figure>".into()))
    }
}

impl<'a, 'b, 'c, I: Iterator<Item=cmark::Event<'b>>> Iterator for CodeImageProcessor<'a, 'b, 'c, I> {
    type Item = cmark::Event<'b>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.len() > 0 { return self.buffer.pop_front() }
        let Some(event) = self.iter.next() else { return None };
        match &event {
            cmark::Event::Start(cmark::Tag::CodeBlock(cmark::CodeBlockKind::Fenced(language))) => {
                let Some(source) = self.accumulate_plain_text(cmark::TagEnd::CodeBlock, "code block") 
                    else { return Some(event); };

                match self.highlighter.highlight(&language, source.trim_end()) {
                    Ok(html) => {
                        let html = format!("<a-lf></a-lf>{}", html.replace('\n', "\n<a-lf></a-lf>"));
                        self.buffer.clear();
                        self.buffer.push_back(cmark::Event::Html(html.into()));
                        self.buffer.push_back(cmark::Event::End(cmark::TagEnd::CodeBlock));
                    },
                    Err(arborium::Error::UnsupportedLanguage { language }) => println!("warning: syntax highlighting is not supported for {}", language),
                    Err(e) => println!("error: could not highlight code: {}", e)
                }

                Some(event)
            },
            cmark::Event::Start(cmark::Tag::Image { dest_url, .. }) => {
                let Some(alt) = self.accumulate_plain_text(cmark::TagEnd::Image, "image") 
                    else { return Some(event); };

                let Err(url::ParseError::RelativeUrlWithoutBase) = url::Url::parse(&dest_url)
                    .inspect_err(|e| if !matches!(e, url::ParseError::RelativeUrlWithoutBase) { 
                        println!("error: cannot parse image url `{}`: {}", dest_url, e); 
                    }) else { return Some(event) };
                
                let Some(path) = self.post.resolve_file(&dest_url) else {
                    println!("error: could not resolve relative file `{}`", dest_url);
                    return Some(event)
                };

                if path.extension().and_then(|e| e.to_str()) == Some("svg") {
                    self.handle_svg_image(path, alt, event)
                } else {
                    self.handle_raster_image(path, alt, event)
                }
            },
            cmark::Event::Start(cmark::Tag::MetadataBlock(cmark::MetadataBlockKind::PlusesStyle)) => {
                let Some(source) = self.accumulate_plain_text(cmark::TagEnd::MetadataBlock(cmark::MetadataBlockKind::PlusesStyle), "metadata")
                    else { return Some(event); };

                let Ok(meta_raw) = toml::from_str::<'_, PostMetaIncomplete>(&source)
                    .inspect_err(|e| {
                        println!("error: could not parse metadata: {}", e);
                    }) else { return Some(event); };

                let meta = PostMeta {
                    title: meta_raw.title.unwrap_or_else(|| self.post.get_default_title()),
                    date: meta_raw.date.unwrap_or_else(|| self.post.get_default_date()),
                    tags: meta_raw.tags.unwrap_or(Vec::new()),
                    ghcomment: meta_raw.ghcommentid.zip(meta_raw.ghcommentauthors)
                };
                println!(
                    "info: got post metadata:\n    title = {:?},\n    date = {},\n    tags = {:?}\n    ghcomment = {:?}", 
                    meta.title, meta.date, meta.tags, meta.ghcomment
                );
                self.post.meta = Some(meta);

                self.buffer.clear();
                self.iter.next()
            },
            _ => Some(event)
        }
    }
}

struct MathProcessor<I> {
    iter: I,
    storage: latex::Storage
}

impl<'a, I: Iterator<Item=cmark::Event<'a>>> Iterator for MathProcessor<I> {
    type Item = cmark::Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(event) = self.iter.next() else { return None };
        match &event {
            cmark::Event::DisplayMath(math) | cmark::Event::InlineMath(math) => {
                let parser = latex::Parser::new(&math, &self.storage);
                let mut buffer = String::new();
                let mut config = latex::RenderConfig::default();
                config.display_mode = match event { 
                    cmark::Event::DisplayMath(_) => latex::config::DisplayMode::Block,
                    _ => latex::config::DisplayMode::Inline
                };
                config.annotation = Some(&math);
                let mut found_mathml_error = Ok(());
                let parser = parser.inspect(|e| {
                    if let Err(e) = e { 
                        found_mathml_error = Err(format!("{}", e));
                    }
                });
                if let Err(e) = latex::push_mathml(&mut buffer, parser, config)
                    .map_err(|e| e.to_string()).and(found_mathml_error) {
                    println!("error: cannot render math block: {}", e);
                    self.iter.next()
                } else {
                    Some(cmark::Event::Html(buffer.into()))
                }
            },
            _ => Some(event)
        }
    }
}

