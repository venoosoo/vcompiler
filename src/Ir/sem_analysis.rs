use std::collections::HashMap;

use crate::Ir::{
    Stmt,
    expr::{BinOp, UnaryOp},
    r#gen::StructData,
    stmt::Type,
};

#[derive(Debug, Clone)]
pub struct ArgData {
    pub arg_name: String,
    pub arg_type: Type,
}

#[derive(Debug, Clone)]
pub struct SemFuncData {
    pub args: Vec<ArgData>,
    pub ret_type: Type,
}

#[derive(Debug, Clone)]
pub struct Analyzer<'a> {
    pub stmts: &'a Vec<Stmt>,
    pub errors: Vec<SemanticError>,
    pub scopes: Vec<HashMap<String, Type>>,
    pub functions: HashMap<String, SemFuncData>,
    pub structs: HashMap<String, StructData>,
    pub current_ret_type: Type,
    // track loop depth for break/continue
    pub loop_depth: usize,
}

#[derive(Debug, Clone)]
pub enum SemanticError {
    UndeclaredVariable(String),
    UndeclaredFunction(String),
    UndeclaredStruct(String),
    UndeclaredField(String, String), // (struct_name, field_name)
    AlreadyDeclared(String),
    TypeMismatch {
        expected: Type,
        got: Type,
    },
    ArgCountMismatch {
        func: String,
        expected: usize,
        got: usize,
    },
    ArgTypeMismatch {
        func: String,
        pos: usize,
        expected: Type,
        got: Type,
    },
    StructCountMismatch {
        struct_name: String,
        expected: usize,
        got: usize,
    },
    StructTypeMismatch {
        struct_name: String,
        expected: Type,
        got: Type,
    },
    StructNameNotFound {
        struct_name: String,
        got: String,
    },
    ReturnTypeMismatch {
        expected: Type,
        got: Type,
    },
    ReturnOutsideFunction,
    NotAPointer(Type),
    NotIndexable(Type),
    NotAStruct(Type),
    InvalidArrayIndex(Type),
    NonArrayIndex(Type),
    InvalidUnary {
        op: UnaryOp,
        ty: Type,
    },
    InvalidBinary {
        op: BinOp,
        left: Type,
        right: Type,
    },
    DerefNonPointer(Type),
    CircularStruct(String),
    MissingReturn(String),
}
