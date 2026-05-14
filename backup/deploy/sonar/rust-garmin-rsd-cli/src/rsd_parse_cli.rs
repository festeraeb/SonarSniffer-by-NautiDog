mod garmin_rsd_parser;
use garmin_rsd_parser::GarminRSDParser;
use std::env;
use std::fs;
use serde_json;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: rsd_parse_cli <RSD_FILE>");
        std::process::exit(1);
    }
    let file_path = &args[1];
    let mut parser = GarminRSDParser::new();
    let result = parser.parse_file(file_path);
    let json = serde_json::to_string_pretty(&result).unwrap();
    println!("{}", json);
    let _ = fs::write("rsd_parse_output.json", &json);
}
