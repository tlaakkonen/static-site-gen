mod post;

use std::{collections::{HashMap, HashSet}, io::Read, path::PathBuf};
use clap::Parser;
use minijinja::context;
use serde::Serialize;
use post::{Post, PostBuilder};

fn parse_dir(s: &str) -> Result<PathBuf, String> {
    let path = std::fs::canonicalize(s).map_err(|err| err.to_string())?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err("The provided path must be a directory".into())
    }
}

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(help="Directory for input files", value_parser=parse_dir)]
    in_dir: PathBuf,
    #[arg(help="Directory for output files", value_parser=parse_dir)]
    out_dir: PathBuf
}

#[derive(Debug)]
pub struct SiteBuilder {
    args: Args,
    assets: HashMap<u64, (Vec<u8>, String)>,
    posts: Vec<Post>,
    env: minijinja::Environment<'static>
}

impl SiteBuilder {
    fn asset_path(hash: u64, ext: &str) -> String {
        format!("assets/{:016x}.{}", hash, ext)
    }

    pub fn store_asset(&mut self, asset: Vec<u8>, ext: &str) -> String {
        let hash = {
            use std::hash::Hasher;
            let mut hasher = std::hash::DefaultHasher::new();
            hasher.write(&asset);
            hasher.finish()
        };

        let ext = &self.assets.entry(hash)
            .or_insert_with(|| (asset, ext.to_string())).1;
        Self::asset_path(hash, ext)
    }

    fn build_posts(&mut self) {
        let Ok(posts_dir) = self.args.in_dir.join("posts").read_dir()
            .inspect_err(|e| {
                println!("error: cannot read posts directory: {e}");
                println!("warning: continuing with no posts");
            }) else { return };

        for entry in posts_dir {
            let Ok(entry) = entry.map(|e| e.path())
                .inspect_err(|e| {
                    println!("error: cannot read post: {e}")
                }) else { continue };

            let builder = if entry.is_dir() {
                let index = entry.join("index.md");
                if index.is_file() {
                    PostBuilder { site: self, file: index, dir: Some(entry), meta: None }
                } else {
                    println!("error: unknown post type for: `{}`", index.display());
                    continue
                }
            } else if entry.is_file() && entry.extension().and_then(|e| e.to_str()) == Some("md") {
                PostBuilder { site: self, file: entry, dir: None, meta: None }
            } else {
                println!("error: unknown post type for `{}`", entry.display());
                continue
            };

            if let Some(post) = builder.build() {
                self.posts.push(post);
            }
        }
    }

    fn load_templates(&mut self) {
        let Ok(templates_dir) = self.args.in_dir.join("templates").read_dir()
            .inspect_err(|e| {
                println!("error: cannot read templates directory: {e}");
            }) else { return };
        
        for entry in templates_dir {
            let Ok(entry) = entry.map(|e| e.path())
                .inspect_err(|e| {
                    println!("error: cannot read template: {e}")
                }) else { continue };

            let Some(name) = entry.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.trim_end_matches(".html")) else {
                    println!("error: unknown template name for: `{}`", entry.display());
                    continue
                };

            println!("info: processing template `{}` at `{}`", name, entry.display());

            let mut source = String::new();
            let Ok(_) = std::fs::File::open(&entry)
                .and_then(|mut file| file.read_to_string(&mut source))
                .inspect_err(|e| {
                    println!("error: cannot read template: {e}")
                }) else { continue };

            if let Err(e) = self.env.add_template_owned(name.to_string(), source) {
                println!("error: cannot parse template: {e}");
            }
        }

        fn format_datetime_function(s: &minijinja::State<'_, '_>, dt: minijinja::value::ViaDeserialize<toml_datetime::Datetime>) -> String {
            let format_value = s.lookup("FORMAT_DATETIME");
            let format = format_value
                .as_ref()
                .and_then(|v| v.as_str())
                .unwrap_or("%B %e %Y at %H:%M");

            let cdt = dt_toml_to_chrono(&dt);
            let readable = cdt.format(format);
            let timestamp = cdt.to_rfc3339();
            format!("<time datetime=\"{}\">{}</time>", timestamp, readable)
        }
        self.env.add_filter("format_datetime", format_datetime_function);
        self.env.add_filter("urlencode", |s: String| urlencoding::encode(&s).to_string());
    }

    fn write_to_output(&self, outpath: &str, content: &[u8]) {
        let target = self.args.out_dir.join(outpath);
        if let Some(parent) = target.parent() {
            let Ok(()) = std::fs::create_dir_all(parent)
                .inspect_err(|e| println!("error: could not write output `{}`: {}", target.display(), e))
                else { return };
        }
        let Ok(_) = std::fs::File::create(&target)
            .and_then(|mut file| {
                use std::io::Write;
                file.write_all(content)
            })
            .inspect_err(|e| println!("error: could not write output `{}`: {}", target.display(), e))
            else { return };
    }

    fn build_pages(&self) {
        self.build_page("index", "index.html", context! { posts => &self.posts });
        
        let mut tags = HashSet::new();
        for post in &self.posts {
            self.build_page("post", &format!("posts/{}.html", post.id), context! { post => post });

            for tag in &post.meta.tags {
                tags.insert(tag.clone());
            }
        }

        for tag in tags {
            self.build_page("tag", &format!("tags/{}.html", tag), context! { posts => &self.posts, tag => tag });
        }

        for (&hash, (content, ext)) in &self.assets {
            println!("info: writing asset {:016x} of type `{}`", hash, ext);
            self.write_to_output(&Self::asset_path(hash, ext), content);
        }
    }

    fn build_page<C: Serialize>(&self, tname: &str, outpath: &str, context: C) {
        println!("info: rendering page `{}` with template `{}`", outpath, tname);

        let Ok(template) = self.env.get_template(tname)
            .inspect_err(|e| println!("error: cannot read template `{}`: {}", tname, e))
            else { return };

        let Ok(source) = template.render(context)
            .inspect_err(|e| println!("error: could not render template `{}`: {}", tname, e))
            else { return };

        self.write_to_output(outpath, source.as_bytes());        
    }

    fn copy_static(&self) {
        let static_in_dir = self.args.in_dir.join("static");
        if !static_in_dir.is_dir() { return }

        let static_out_dir = self.args.out_dir.join("static");
        let Ok(()) = std::fs::create_dir_all(&static_out_dir)
            .inspect_err(|e| println!("error: could not create static directory: {e}"))
            else { return };
        
        for entry in walkdir::WalkDir::new(&static_in_dir) {
            let Ok(entry) = entry
                .inspect_err(|e| {
                    println!("error: could not read static asset: {e}")
                }) else { continue };
            if !entry.file_type().is_file() { continue }

            println!("info: copying static asset `{}`", entry.path().display());

            let Ok(relpath) = entry.path().strip_prefix(&static_in_dir) else { continue };
            let target = static_out_dir.join(relpath);

            if entry.depth() > 1 && let Some(parent) = target.parent() {
                let Ok(()) = std::fs::create_dir_all(parent)
                    .inspect_err(|e| println!("error: could not copy static asset: {e}"))
                    else { continue };
            }
            if let Err(e) = std::fs::copy(entry.path(), &target) {
                println!("error: could not copy static asset: {e}");
            }
        }
    }
}

pub fn dt_toml_to_chrono(dt: &toml_datetime::Datetime) -> chrono::DateTime<chrono::FixedOffset> {
    (|| {
        let date = chrono::NaiveDate::from_ymd_opt(dt.date?.year as i32, dt.date?.month as u32, dt.date?.day as u32)?;
        let datetime = (|| date.and_hms_opt(dt.time?.hour as u32, dt.time?.minute as u32, dt.time?.second as u32))()
            .unwrap_or(date.and_time(chrono::NaiveTime::MIN));
        let mapped = (|| datetime.and_local_timezone(chrono::FixedOffset::east_opt(match dt.offset? {
            toml_datetime::Offset::Z => 0,
            toml_datetime::Offset::Custom { minutes } => (minutes as i32) * 60
        })?).single())().unwrap_or(datetime.and_utc().fixed_offset());
        Some(mapped)
    })().unwrap_or(chrono::DateTime::UNIX_EPOCH.fixed_offset())
}


fn main() {
    let args = Args::parse();

    let mut builder = SiteBuilder { args, assets: HashMap::new(), posts: Vec::new(), env: minijinja::Environment::new() };
    builder.build_posts();
    builder.load_templates();
    builder.build_pages();
    builder.copy_static();
}
