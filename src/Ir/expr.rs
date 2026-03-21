use crate::{Ir::Stmt, Tokenizer::Token};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(i64),
    Float(f64),
    Variable(String),

    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },

    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },

    Call {
        name: String,
        args: Vec<Expr>,
    },

    StructInit {
        struct_name_ty: String,
        fields: Vec<(String, Expr)>,
    },

    StructMember {
        base: Box<Expr>,
        name: String,
    },

    Deref(Box<Expr>),
    AddressOf(Box<Expr>),

    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    ArrayInit {
        elements: Vec<Expr>,
    },
    SizeOf {
        ty: Box<Stmt>,
    },
    String {
        str: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}
