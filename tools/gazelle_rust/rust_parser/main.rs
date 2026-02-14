//! Rust parser CLI for Gazelle.

use clap::Parser;
use prost::Message;
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;

use gazelle_rust_proto::{ParseRequest, ParseResponse};
use tools__gazelle_rust__rust_parser::parser::{SourceInfo, parse_source};

#[derive(clap::Parser)]
#[command(name = "rust_parser")]
#[command(about = "Parse Rust source files for Gazelle")]
enum Args {
    /// Parse a single file and print results (useful for debugging).
    Parse { path: PathBuf },
    /// Run as IPC server for Gazelle.
    Serve,
}

fn parse_file(path: &Path) -> Result<SourceInfo, Box<dyn Error>> {
    let mut file = match File::open(path) {
        Err(err) => {
            eprintln!(
                "Could not open file {}: {}",
                path.to_str().unwrap_or("<utf-8 decode error>"),
                err,
            );
            std::process::exit(1);
        }
        file => file?,
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    parse_source(&contents)
}

fn handle_parse_request(request: ParseRequest) -> ParseResponse {
    let path = PathBuf::from(request.file_path);
    match parse_file(&path) {
        Ok(result) => ParseResponse {
            success: true,
            error_msg: String::new(),
            imports: result.imports,
            external_modules: result.external_modules,
            has_main: result.has_main,
        },
        Err(err) => ParseResponse {
            success: false,
            error_msg: err.to_string(),
            imports: vec![],
            external_modules: vec![],
            has_main: false,
        },
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match args {
        Args::Parse { path } => {
            let result = parse_file(&path)?;
            println!("imports: {:?}", result.imports);
            println!("external_modules: {:?}", result.external_modules);
            println!("has_main: {}", result.has_main);
        }
        Args::Serve => {
            let mut stdin = std::io::stdin();
            let mut stdout = std::io::stdout();
            let mut buf: Vec<u8> = vec![0; 1024];

            loop {
                // Read 4-byte little-endian size prefix.
                match stdin.read_exact(&mut buf[..4]) {
                    Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                        break;
                    }
                    res => res?,
                }
                let size = u32::from_le_bytes(buf[..4].try_into()?) as usize;

                if size > buf.len() {
                    buf.resize(size, 0);
                }

                stdin.read_exact(&mut buf[..size])?;
                let request = ParseRequest::decode(&buf[..size])?;

                let response = handle_parse_request(request);

                // Write response with size prefix.
                let response_bytes = response.encode_to_vec();
                let size =
                    u32::try_from(response_bytes.len()).expect("response exceeds u32::MAX bytes");
                let size_bytes = size.to_le_bytes();
                stdout.write_all(&size_bytes)?;
                stdout.write_all(&response_bytes)?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}
