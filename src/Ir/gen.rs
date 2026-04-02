use std::collections::{HashMap, HashSet};

use crate::Ir::{
    Stmt,
    stmt::{Declaration, EnumData, StructField, Type},
};

#[derive(Debug, Clone)]
pub struct VarData {
    pub stack_pos: usize,
    pub var_type: Type,
    pub global_flag: bool,
}

pub struct Gen {
    pub stmts: Vec<Stmt>,
    pub stack_pos: usize,
    pub out: String,
    pub current_return_type: Type,
    pub main_code: Vec<String>,
    pub data_code: Vec<String>,
    pub highest_stack_pos: usize,
    pub generic_func: HashMap<String, Stmt>,
    pub generics: HashMap<String, Type>,
    pub scopes: Vec<HashMap<String, VarData>>,
    pub global_vars: HashMap<String, VarData>,
    pub func_header: String,
    pub func_data: String,
    pub func_out: String,
    pub structs: HashMap<String, StructData>,
    pub functions: HashMap<String, Vec<FuncData>>,
    pub enums: HashMap<String, EnumData>,
    pub id: usize,
}

#[derive(Debug, Clone)]
pub struct FuncData {
    pub args: Vec<Declaration>,
    pub generic: Vec<String>,
    // return type and pointer depth
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct StructData {
    // TODO: make this vec because we need order
    pub elements: HashMap<String, StructField>,
    pub name: String,
    pub generic_type: Vec<String>,
    pub byte_size: usize,
}

#[derive(Clone, Debug)]
pub enum Addr {
    Stack(isize), // [rbp - offset]
    Reg(String),  // register holds computed address
}
