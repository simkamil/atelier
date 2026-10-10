// SPDX-License-Identifier: AGPL-3.0-or-later
//! Text-hex adapter used only to compare the native Rust codec with JavaScript.

use atelier_core_wasm::protocol::{Direction, Message, MessageType, decode, encode};
use std::env;
use std::process::ExitCode;

fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    if !input.is_ascii() {
        return Err("hex input must contain ASCII characters only".into());
    }
    if input.len() % 2 != 0 {
        return Err("hex input must have an even number of characters".into());
    }
    (0..input.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&input[index..index + 2], 16).map_err(|error| error.to_string())
        })
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn direction(input: &str) -> Result<Direction, String> {
    match input {
        "command" => Ok(Direction::Command),
        "event" => Ok(Direction::Event),
        _ => Err("direction must be command or event".into()),
    }
}

fn run() -> Result<String, String> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("encode") if args.len() == 5 => {
            let kind_value = u16::from_str_radix(&args[2], 16).map_err(|error| error.to_string())?;
            let kind = MessageType::from_u16(kind_value).ok_or("unknown message type")?;
            let request_id = args[3].parse::<u64>().map_err(|error| error.to_string())?;
            let payload = parse_hex(&args[4])?;
            let bytes = encode(&Message { kind, request_id, payload }).map_err(|error| error.to_string())?;
            Ok(to_hex(&bytes))
        }
        Some("decode") if args.len() == 4 => {
            let dir = direction(&args[2])?;
            let bytes = parse_hex(&args[3])?;
            let message = decode(&bytes, dir).map_err(|error| error.to_string())?;
            Ok(format!("{:04x} {} {}", message.kind as u16, message.request_id, to_hex(&message.payload)))
        }
        _ => Err("usage: protocol_oracle encode TYPE_HEX REQUEST_ID PAYLOAD_HEX | decode command|event FRAME_HEX".into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
