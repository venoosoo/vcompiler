use clap::Parser as CliParser;
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Write},
    path::Path,
};

use std::fs;

use crate::Ir::sem_analysis::Analyzer;

mod Gen;
mod Ir;
mod Parser;
mod tokenizer;
mod llvm_gen;
mod sem_analysis;

#[derive(CliParser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(short, long, required = true, help = "provide file main.v")]
    file: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli: Cli = Cli::parse();

    let mut file = File::open(cli.file.clone())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let mut tokenizer = tokenizer::Tokenizer::new(contents);
    tokenizer.tokenize();

    println!("{}", tokenizer);

    let file_path = Path::new(&cli.file);
    let base_dir = file_path.parent().unwrap().to_path_buf();

    let mut imported_files = HashSet::new();
    let mut parser = Parser::Parser::new(tokenizer.m_res, base_dir.clone(), &mut imported_files);
    let res = parser.parse();

    let mut file = File::create("parser_result.txt").expect("Failed to create parser_result.txt");

    write!(file, "parse result\n{:#?}", res).expect("Failed to write to file");
    let mut analyzer = Analyzer::new(&res);
    analyzer.check_code();
    if analyzer.errors.len() > 0 {
        let error_dump = format!("{:#?}", analyzer.errors);
        fs::write("errors.txt", error_dump).expect("Failed to write errors.txt");
        // panic!("errors");
    }

    let mut generator = crate::Ir::r#gen::Gen::new(res);
    let asm = generator.gen_asm()?;
    let mut file = File::create("main.asm")?;
    let _res = file.write(asm.as_bytes())?;

    Ok(())
}
