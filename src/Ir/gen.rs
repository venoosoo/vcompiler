use std::collections::HashMap;

use crate::{
    Ir::{
        Stmt,
        stmt::{Arg, StructField, Type},
    },
    Tokenizer::TokenType,
};

#[derive(Debug)]
pub struct VarData {
    pub stack_pos: usize,
    pub var_type: Type,
}

#[derive(Debug, Clone)]
pub struct FuncData {
    pub args: Vec<Stmt>,
    // return type and pointer depth
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct StructData {
    pub elements: HashMap<String, StructField>,
    pub element_size: usize,
}

#[derive(Clone)]
pub enum Addr {
    Stack(isize), // [rbp - offset]
    Reg(String),  // register holds computed address
}
