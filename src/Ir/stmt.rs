use std::collections::HashMap;

use crate::Ir::expr::Expr;
use crate::Tokenizer::TokenType;

#[derive(Debug, Clone, PartialEq)]
pub enum LValue {
    Variable(String),
    Field { base: Box<LValue>, name: String },
    Deref(Box<LValue>),
    Index { base: Box<LValue>, index: Box<Expr> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Primitive(TokenType),
    Pointer(Box<Type>),
    Array(Box<Type>, usize),
    Struct(String),
    Enum(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub ty: Type,
    pub initializer: Option<Expr>,
}

/// Statements
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Block(Vec<Stmt>), // scopes
    Declaration(Declaration),
    Assignment {
        target: LValue,
        value: Expr,
    },
    ExprStmt(Expr), // function calls or standalone expressions

    If {
        condition: Expr,
        if_block: Box<Stmt>,
        else_block: Option<Box<Stmt>>,
    },

    While {
        condition: Expr,
        body: Box<Stmt>,
    },

    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Box<Stmt>>,
        body: Box<Stmt>,
    },

    Return(Option<Expr>),
    AsmCode(Vec<String>),
    InitFunc {
        name: String,
        args: Vec<Declaration>,
        ret_type: Type,
        data: Box<Stmt>,
    },
    InitStruct(StructDef),
    GlobalDecl(Box<Stmt>),
    InitEnum {
        name: String,
        variants: HashMap<String, EnumVariant>,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub offset: usize,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub tag: usize,
    pub args: Vec<Declaration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumData {
    pub name: String,
    pub variants: HashMap<String, EnumVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<StructField>,
    pub size: usize,
}
