use std::collections::HashMap;

use crate::Ir::{
    Stmt,
    stmt::{Declaration, StructField, Type},
};

#[derive(Debug, Clone)]
pub struct VarData {
    pub stack_pos: usize,
    pub var_type: Type,
    pub global_flag: bool,
}

#[derive(Debug, Clone)]
pub struct FuncData {
    pub args: Vec<Declaration>,
    // return type and pointer depth
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct StructData {
    pub elements: HashMap<String, StructField>,
    pub byte_size: usize,
}

#[derive(Clone, Debug)]
pub enum Addr {
    Stack(isize), // [rbp - offset]
    Reg(String),  // register holds computed address
}
