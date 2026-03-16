use clap::Parser as CliParser;
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use std::fs;

use crate::Ir::sem_analysis::Analyzer;

mod Gen;
mod Ir;
mod Parser;
mod Tokenizer;
mod sem_analysis;

#[derive(CliParser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(short, long, required = true, help = "provide file main.v")]
    file: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli: Cli = Cli::parse();

    println!("file name is: {}", cli.file);

    let mut file = File::open(cli.file.clone())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    println!("file contains: {}", &contents);

    let mut tokenizer = Tokenizer::Tokenizer::new(contents);
    tokenizer.tokenize();
    println!("{}", tokenizer);

    let mut parser = Parser::Parser::new(tokenizer.m_res);
    let res = parser.parse();

    let mut analyzer = Analyzer::new(&res);
    analyzer.check_code();
    let error_dump = format!("{:#?}", analyzer.errors);
    fs::write("errors.txt", error_dump).expect("Failed to write errors.txt");

    // to lazy to make normal debug print
    let mut file = File::create("parser_result.txt").expect("Failed to create parser_result.txt");

    write!(file, "parse result\n{:#?}", res).expect("Failed to write to file");

    let file_path = Path::new(&cli.file);
    let base_dir = file_path.parent().unwrap().to_path_buf();

    let mut generator = Gen::Gen::new(res, base_dir);
    let asm = generator.gen_asm()?;
    let mut file = File::create("main.asm")?;
    let _res = file.write(asm.as_bytes())?;

    Ok(())
}
