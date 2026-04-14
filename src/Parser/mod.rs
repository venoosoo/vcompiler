use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::ExitCode;

use crate::Ir::expr::{BinOp, Expr, UnaryOp};
use crate::tokenizer::{Token, TokenType};

use crate::Ir::stmt::{EnumData, EnumVariant, MatchLeftValue, Stmt, StructDef, Type};

pub mod expr;
pub mod function;
pub mod stmt;

pub struct Parser<'a> {
    m_tokens: Vec<Token>,
    m_index: usize,
    expressions: Vec<Stmt>,
    struct_table: HashMap<String, StructDef>,
    types: HashSet<String>,
    enums_table: HashMap<String, EnumData>,
    base_dir: PathBuf,
    generic: HashSet<String>,
    imported_files: &'a mut HashSet<String>,
}

pub struct Program(Vec<Stmt>);

impl std::fmt::Display for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, expr) in self.0.iter().enumerate() {
            writeln!(f, "[{}] {:?}", i, expr)?;
        }
        Ok(())
    }
}

impl<'a> Parser<'a> {
    pub fn new(
        m_tokens: Vec<Token>,
        base_dir: PathBuf,
        imported_files: &'a mut HashSet<String>,
    ) -> Self {
        Parser {
            m_tokens,
            m_index: 0,
            struct_table: HashMap::new(),
            expressions: Vec::new(),
            types: HashSet::new(),
            generic: HashSet::new(),
            enums_table: HashMap::new(),
            base_dir,
            imported_files,
        }
    }

    fn peek(&self, offset: usize) -> &Token {
        let pos: usize = self.m_index + offset;
        if self.m_index >= self.m_tokens.len() {
            panic!(
                "Trying to parse token more than token array has\nm_index: {}",
                self.m_tokens.len()
            );
        }
        &self.m_tokens[pos]
    }

    pub fn expect(&mut self, ty: TokenType) -> Option<bool> {
        if self.peek(0).token != ty {
            return None;
        }
        self.consume();
        Some(true)
    }

    fn consume(&mut self) -> Token {
        if self.m_index >= self.m_tokens.len() {
            panic!("Trying to consume more than m_src len");
        }
        self.m_tokens.remove(0)
    }

    fn is_struct(&self, var_name: &String) -> bool {
        if self.types.get(var_name).is_some() {
            return true;
        }
        return false;
    }

    pub fn parse(&mut self) -> Vec<Stmt> {
        while !self.m_tokens.is_empty() {
            if let Some(stmt) = self.parse_stmt() {
                self.expressions.push(stmt);
            } else {
                println!("tokens: {:?}", self.m_tokens);
                panic!(
                    "Unexpected token: {:?} at {}",
                    self.peek(0).token,
                    self.m_index
                );
            }
        }

        self.expressions.clone()
    }

    fn size_of(&self, ty: &Type) -> usize {
        match ty {
            Type::Primitive(TokenType::IntType) => 4,
            Type::Primitive(TokenType::CharType) => 1,
            Type::Primitive(TokenType::ShortType) => 2,
            Type::Primitive(TokenType::LongType) => 8,

            Type::Pointer(_) => 8, // assume 64-bit

            Type::Array(inner, count) => self.size_of(inner) * count,

            Type::Struct(name) => {
                self.struct_table
                    .get(name)
                    .expect(&format!("unkown struct: {}", name))
                    .size
            }
            Type::Enum(_) => 8,
            Type::GenericType(_) => 0,
            _ => {
                println!("ty: {:?}", ty);
                panic!("Unknown type size")
            }
        }
    }

    fn get_custom_type(&mut self, name: &String) -> Option<Type> {
        if self.types.contains(name) {
            if self.struct_table.contains_key(name) {
                return Some(Type::Struct(name.clone()));
            } else if self.enums_table.contains_key(name) {
                return Some(Type::Enum(name.clone()));
            } else {
                return Some(Type::GenericType(name.clone()));
            }
        }
        None
    }
}
