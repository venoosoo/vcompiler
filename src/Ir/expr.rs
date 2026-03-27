use std::collections::HashMap;

use crate::{
    Ir::{Stmt, stmt::Type},
    Tokenizer::Token,
};

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
    GetEnum {
        base: String,
        variant: String,
        value: Vec<EnumExprField>,
    },
    Cast {
        expr: Box<Expr>,
        ty: Type,
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

#[derive(Debug, Clone, PartialEq)]
pub struct EnumExprField {
    pub name: String,
    pub expr: Expr,
}

pub trait Lookup {
    fn look_var(&self, name: &String) -> Type;
    fn look_unary(&self, op: &UnaryOp, expr: &Box<Expr>) -> Type;
    fn look_binary(&self, op: &BinOp, left: &Box<Expr>, right: &Box<Expr>) -> Type;
    fn look_struct_init(&self, struct_name: &String) -> Type;
    fn look_deref(&self, ptr_expr: &Box<Expr>) -> Type;
    fn look_addres_of(&self, var_expr: &Box<Expr>) -> Type;
    fn look_index(&self, base: &Box<Expr>, index: &Box<Expr>) -> Type;
    fn look_struct_member(&self, base: &Box<Expr>, name: &String) -> Type;
    fn look_call(&self, name: &String, arg: &Vec<Expr>) -> Type;
    fn look_array_init(&self, elements: &Vec<Expr>) -> Type;
    fn look_get_enum(&self, base: &String) -> Type;
}
