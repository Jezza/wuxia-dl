#![recursion_limit = "1024"]

#![feature(alloc_system)]
extern crate alloc_system;
extern crate epub_builder;
#[macro_use]
extern crate error_chain;
extern crate regex;
extern crate reqwest;
extern crate select;
extern crate url;

use epub_builder::EpubBuilder;
use epub_builder::EpubContent;
use epub_builder::ReferenceType;
use epub_builder::ZipLibrary;
use regex::Regex;
use select::document::Document;
use select::predicate::{Class, Name, Predicate};
use self::errors::*;
use std::env::args;
use std::fs::{File, remove_file};
use std::io::Cursor;
use std::path::Path;
use url::Url;

mod errors {
	error_chain! {}
}

fn main() {
	let args: Vec<String> = args().collect();
	let program = &args[0];

	if args.len() != 2 {
		println!("Usage: {} <url>", program);
		return;
	}

	if let Err(e) = run(args) {
		use std::io::Write;
		use error_chain::ChainedError;
		let stderr = &mut ::std::io::stderr();

		writeln!(stderr, "{}", e.display_chain()).expect("Error writing to stderr");
		::std::process::exit(1);
	}
}

fn run(args: Vec<String>) -> Result<()> {
	let url = &args[1];
	let url = url.parse::<Url>()
				 .chain_err(|| format!("Unable to parse URL: \"{}\"", url))?;

	println!("Downloading: {}", url);
	let data = fetch_data(url);

	let zip = ZipLibrary::new().unwrap();
	let mut builder: EpubBuilder<ZipLibrary> = EpubBuilder::new(zip)
		.chain_err(|| "Unable to construct EpubBuilder")?;
	builder.metadata("title", data.title.clone())
		   .chain_err(|| "Unable to alter title.")?;
	builder.metadata("toc_name", data.title.clone())
		   .chain_err(|| "Unable to alter Table of Contents.")?;
	builder.metadata("author", "WuxiaWorld")
		   .chain_err(|| "Unable to set author metadata.")?;

	let size = data.chapters.len();

	for chapter in data.chapters {
		let index = chapter.index;
		println!("Fetching {}/{} :: \"{}\".", index, size, chapter.title);

		let page = fetch_chapter_data(chapter)
			.chain_err(|| "Unable to fetch chapter content")?;

		builder.add_content(page)
			   .chain_err(|| format!("Unable to add page: {}/{}", index, size))?;
	}

	let path = format!("{}.epub", data.title);
	let path = Path::new(&path);
	if path.exists() {
		println!("File (\"{}\") already exists. Deleting...", path.display());
		remove_file(path)
			.chain_err(|| format!("Failed to remove previous file: \"{}\"", path.display()))?;
	}
	let file = File::create(path)
		.chain_err(|| format!("Unable to create file: \"{}\"", path.display()))?;
	builder.generate(file)
		   .chain_err(|| "Unable to generate epub")?;

	println!("Generated epub file @ \"{}\" for \"{}\"", path.display(), data.title);

	Ok(())
}

fn fetch_data(url: Url) -> BookData {
	let mut res = reqwest::get(url).unwrap();

	let chapter_regex = Regex::new(r".+?(\d+)[- ]*(.*)").unwrap();

	let doc = Document::from_read(&mut res).unwrap();

	let url = res.url();

	let book_title = doc.find(Class("p-15").descendant(Name("h4"))).next().unwrap().text();

	let mut chapters = Vec::new();
	for node in doc.find(Class("chapter-item").descendant(Name("a"))) {
		let full_title = node.text().trim().to_owned();

		let cap = chapter_regex.captures(&full_title).unwrap();

		let index = cap[1].parse::<u32>().unwrap();
		let title = cap[2].to_owned();

		let link = url.join(node.attr("href").unwrap()).unwrap();

		chapters.push(Chapter {
			index,
			title,
			link,
		});
	}

	let data = BookData {
		title: book_title,
		chapters,
	};

	println!("Found \"{}\" with {} chapters at \"{}\"", data.title, data.chapters.len(), url);

	data
}

fn fetch_chapter_data(chapter: Chapter) -> Result<EpubContent<Cursor<String>>> {
	let mut res = reqwest::get(chapter.link.clone())
		.chain_err(|| "Unable to send get request.")?;
//	let text = res.text()
//				  .chain_err(|| "Unable to read get request.")?;
//	let mut cursor = Cursor::new(text.clone());

	let doc = Document::from_read(&mut res)
		.chain_err(|| "Invalid content from request")?;

	let mut content = String::new();
	for node in doc.find(Class("fr-view").descendant(Name("span"))) {
		content.push_str(&node.text());
		content.push_str(&"<br><br>");
		content.push(' ');
	}
	content.pop();
	if content.len() == 0 {
		for node in doc.find(Class("fr-view").child(Name("p"))) {
			content.push_str(&node.text());
			content.push_str(&"<br><br>");
			content.push(' ');
		}
	}
	if content.len() == 0 {
		panic!("Discoverd no content for \"Chapter {} - {}\"", chapter.index, chapter.title);
	}

	let name = format!("chapter_{}.xhtml", chapter.index);
	let chapter_title = format!("Chapter {}", chapter.index);

	let cursor = Cursor::new(content);

	Ok(EpubContent::new(name, cursor)
		.title(chapter_title)
		.reftype(ReferenceType::Text))
}

#[derive(Debug)]
struct BookData {
	title: String,
	chapters: Vec<Chapter>,
}

#[derive(Debug)]
struct Chapter {
	index: u32,
	title: String,
	link: Url,
}