// SPDX-License-Identifier: AGPL-3.0-or-later
//! Agent-only interoperability driver; compiled by check-format-oracle.py.
#![forbid(unsafe_code)]
use std::io::{self, Read};
use atelier_xml::{Event, Reader, Writer as XmlWriter};
use atelier_zip::{Archive, Limits, Method, WriteOptions, Writer as ZipWriter};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
struct Fragment { bytes: Vec<u8>, at: usize, chunk: usize }
impl Read for Fragment {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(self.chunk).min(self.bytes.len() - self.at);
        out[..n].copy_from_slice(&self.bytes[self.at..self.at+n]);
        self.at += n;
        Ok(n)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "xml" => {
            let input = Fragment { bytes: std::fs::read(&args[2])?, at: 0, chunk: args[3].parse()? };
            let mut reader = Reader::new(input);
            let mut writer = XmlWriter::new(Vec::new())?;
            while let Some(event) = reader.next_event()? {
                writer.write_event(&event)?;
                match event {
                    Event::StartElement { name, attributes, empty } => {
                        let mut attrs: Vec<_> = attributes.iter().map(|a|
                            format!("{}:{}:{}",hex(a.name.expanded().0.as_bytes()),hex(a.name.local.as_bytes()),hex(a.value.as_bytes()))).collect();
                        attrs.sort();
                        println!("S {}:{} {}", hex(name.expanded().0.as_bytes()), hex(name.local.as_bytes()), attrs.join(" "));
                        if empty { println!("E"); }
                    },
                    Event::EndElement { .. } => println!("E"),
                    Event::Text(text) => println!("T {}", hex(text.as_bytes())),
                    _ => (),
                }
            }
            let bytes = writer.finish()?;
            if args.len() > 4 { std::fs::write(&args[4], bytes)?; }
        },
        "zip-read" => {
            let bytes = std::fs::read(&args[2])?;
            let archive = Archive::open(&bytes, Limits {max_compression_ratio: 100_000, ..Limits::default()})?;
            println!("COMPLETE {}", archive.is_complete);
            for e in archive.entries() {
                match e.read_all() {
                    Ok(bytes) => println!("OK {} {} {}", hex(e.name().as_bytes()), e.method(), hex(&bytes)),
                    Err(error) => println!("ERR {} {:?}", hex(e.name().as_bytes()), error),
                }
            }
        },
        "zip-write" => {
            let mut writer = ZipWriter::new(WriteOptions { force_zip64: args[3] == "64", ..WriteOptions::default() });
            for e in args[4..].chunks_exact(3) {
                writer.add(&e[0], &std::fs::read(&e[2])?, if e[1] == "0" {Method::Store} else {Method::Deflate})?;
            }
            std::fs::write(&args[2], writer.finish()?)?;
        },
        _ => return Err("unknown command".into()),
    }
    Ok(())
}
