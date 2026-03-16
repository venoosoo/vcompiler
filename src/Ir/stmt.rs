use crate::Ir::expr::Expr;
use crate::Tokenizer::TokenType;

#[derive(Debug, Clone)]
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
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Declaration {
    pub name: String,
    pub ty: Type,
    pub initializer: Option<Expr>,
}

/// Statements
#[derive(Debug, Clone)]
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
        args: Vec<Stmt>,
        ret_type: Type,
        data: Box<Stmt>,
    },
    InitStruct(StructDef),
    Import(String),
}

/// Function argument
#[derive(Debug, Clone)]
pub struct Arg {
    pub name: String,
    pub ty: Type,
}

/// Function definition
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub args: Vec<Arg>,
    pub return_type: Type,
    pub body: Stmt, // usually a Block
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub offset: usize,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<StructField>,
    pub size: usize,
}
