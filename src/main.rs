#![recursion_limit = "1024"]

extern crate epub_builder;
#[macro_use]
extern crate error_chain;
extern crate rayon;
extern crate regex;
extern crate reqwest;
extern crate select;
extern crate url;

use epub_builder::EpubBuilder;
use epub_builder::EpubContent;
use epub_builder::ReferenceType;
use epub_builder::ZipLibrary;
use rayon::prelude::*;
use regex::Regex;
use reqwest::Client;
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

	let client = Client::new();

	println!("Inspecting \"{}\"...", url);
	let info: BookInfo = fetch_book_info(&client, url)
		.chain_err(|| format!("Unable to fetch book info."))?;

	let zip = ZipLibrary::new()
		.chain_err(|| "Unable to construct ZipLibrary.")?;
	let mut builder: EpubBuilder<ZipLibrary> = EpubBuilder::new(zip)
		.chain_err(|| "Unable to construct EpubBuilder")?;
	builder.metadata("title", info.title.clone())
		   .chain_err(|| "Unable to alter title.")?;
	builder.metadata("toc_name", info.title.clone())
		   .chain_err(|| "Unable to alter Table of Contents.")?;
	builder.metadata("author", "WuxiaWorld")
		   .chain_err(|| "Unable to set author metadata.")?;

	let size = info.chapters.len();

	let pages: Vec<EpubContent<Cursor<String>>> = info.chapters
				  .into_par_iter()
				  .map(|chapter| {
					  fetch_chapter_content(&client, chapter, size)
						  .chain_err(|| "Unable to fetch chapter content")
						  .unwrap()
				  })
				  .collect();

	let mut index = 0;
	for page in pages {
		builder.add_content(page)
			   .chain_err(|| format!("Unable to add page: {}/{}", index, size))?;
		index += 1;
	}

	let path = format!("{}.epub", info.title);
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

	println!("Generated epub file @ \"{}\" for \"{}\"", path.display(), info.title);

	Ok(())
}

fn fetch_book_info(client: &Client, url: Url) -> Result<BookInfo> {
	let req = client.get(url)
					.build()
					.chain_err(|| "Unable to construct book info request.")?;
	let mut res = client.execute(req)
						.chain_err(|| "Unable to execute book info request.")?;

	let chapter_regex = Regex::new(r".+?(\d+)[- ]*(.*)")
		.chain_err(|| "Unable to construct regex.")?;

	let doc = Document::from_read(&mut res)
		.chain_err(|| "Unable to construct document from response.")?;

	let url = res.url();

	let book_title = doc.find(Class("p-15").descendant(Name("h4"))).next()
						.chain_err(|| "Failed to locate book title")?
		.text();

	let mut chapters = Vec::new();
	for node in doc.find(Class("chapter-item").descendant(Name("a"))) {
		let full_title = node.text().trim().to_owned();

		let cap = chapter_regex.captures(&full_title)
							   .chain_err(|| format!("Failed to match regex against: {}", full_title))?;

		let raw_index = &cap[1];
		let index = raw_index.parse::<u32>()
							 .chain_err(|| format!("Unable to parse index {}", raw_index))?;
		let title = cap[2].to_owned();

		let href = node.attr("href")
					   .chain_err(|| "No href specified")?;
		let link = url.join(href)
					  .chain_err(|| format!("Unable to append href (\"{}\") to url (\"{}\").", href, url))?;

		chapters.push(Chapter {
			index,
			title,
			link,
		});
	}

	let info = BookInfo {
		title: book_title,
		chapters,
	};

	println!("Found \"{}\" with {} chapters at \"{}\"", info.title, info.chapters.len(), url);

	Ok(info)
}

fn fetch_chapter_content(client: &Client, chapter: Chapter, size: usize) -> Result<EpubContent<Cursor<String>>> {
	let req = client.get(chapter.link)
					.build()
					.chain_err(|| "Unable to construct chapter request.")?;

	println!("Fetching {}/{} :: \"{}\".", chapter.index, size, chapter.title);

	let mut res = client.execute(req)
						.chain_err(|| "Unable to send chapter request.")?;

	let doc = Document::from_read(&mut res)
		.chain_err(|| "Invalid content from request")?;

	let mut content = String::new();
	for node in doc.find(Class("fr-view").descendant(Name("span"))) {
		content.push_str(&node.text());
		content.push_str(&"<br><br> ");
	}
	if content.len() == 0 {
		for node in doc.find(Class("fr-view").child(Name("p"))) {
			content.push_str(&node.text());
			content.push_str(&"<br><br> ");
		}
	}
	if content.len() == 0 {
		panic!("Discovered no content for \"Chapter {} - {}\"", chapter.index, chapter.title);
	}

	let name = format!("chapter_{}.xhtml", chapter.index);
	let chapter_title = format!("Chapter {}", chapter.index);

	let cursor = Cursor::new(content);

	Ok(EpubContent::new(name, cursor)
		.title(chapter_title)
		.reftype(ReferenceType::Text))
}

#[derive(Debug)]
struct BookInfo {
	title: String,
	chapters: Vec<Chapter>,
}

#[derive(Debug)]
struct Chapter {
	index: u32,
	title: String,
	link: Url,
}