use std::{cell::RefCell, collections::HashMap};

use indexmap::IndexMap;

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
    pub contniue_stack: Vec<String>,
    pub break_stack: Vec<String>,
    pub highest_stack_pos: usize,
    pub generic_func: HashMap<String, Vec<Stmt>>,
    pub generics: RefCell<HashMap<String, Type>>,
    pub scopes: Vec<HashMap<String, VarData>>,
    pub global_vars: HashMap<String, VarData>,
    pub func_header: String,
    pub func_data: String,
    pub func_out: String,
    pub bss_code: Vec<String>,
    pub structs: RefCell<HashMap<String, StructData>>,
    pub functions: HashMap<String, Vec<FuncData>>,
    pub enums: RefCell<HashMap<String, EnumData>>,
    pub id: usize,
}

#[derive(Debug, Clone)]
pub struct FuncData {
    pub args: Vec<Declaration>,
    pub generic: Vec<String>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct StructData {
    pub elements: IndexMap<String, StructField>,
    pub name: String,
    pub generic_type: Vec<String>,
    pub size: usize, // size of struct
}

#[derive(Clone, Debug)]
pub enum Addr {
    Stack(isize), // [rbp - offset]
    Reg(String),  // register holds computed address
}
