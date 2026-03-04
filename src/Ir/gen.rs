use std::collections::HashMap;

use crate::{Ir::{Stmt, stmt::{Arg, StructField, Type}}, Tokenizer::TokenType};


#[derive(Debug)]
pub(crate) struct VarData {
    pub(crate) stack_pos: usize,
    pub(crate) var_type: Type,
}

#[derive(Debug, Clone)]
pub(crate) struct FuncData {
    pub(crate) args: Vec<Stmt>,
    // return type and pointer depth
    pub(crate) return_type: Type,
}

#[derive(Debug, Clone)]
pub(crate) struct StructData {
    pub(crate) elements: HashMap<String, StructField>,
    pub(crate) element_size: usize,
}

#[derive(Clone)]
pub enum Addr {
    Stack(isize),      // [rbp - offset]
    Reg(String), // register holds computed address
}