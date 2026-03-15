use std::collections::HashMap;

use crate::{
    Ir::{
        Stmt,
        r#gen::StructData,
        sem_analysis::*,
        stmt::{StructField, Type},
    },
    Tokenizer::TokenType,
};

use crate::Gen::type_size;

pub mod sem_expr;
mod sem_stmt;

fn numeric_rank(ty: &Type) -> Option<u8> {
    match ty {
        //Type::Primitive(TokenType::Bool)  => Some(0),
        Type::Primitive(TokenType::CharType) => Some(1),
        Type::Primitive(TokenType::IntType) => Some(2),
        Type::Primitive(TokenType::LongType) => Some(3),
        //Type::Primitive(TokenType::Float) => Some(4),
        _ => None,
    }
}

fn is_numeric(ty: &Type) -> bool {
    numeric_rank(ty).is_some()
}

fn is_arithmetic(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(TokenType::IntType) | Type::Primitive(TokenType::LongType) //Type::Primitive(TokenType::Float)
    )
}

fn is_integer(ty: &Type) -> bool {
    matches!(
        ty,
        //Type::Primitive(TokenType::Bool)  |
        Type::Primitive(TokenType::CharType)
            | Type::Primitive(TokenType::IntType)
            | Type::Primitive(TokenType::LongType)
    )
}

fn coerce_numeric(a: &Type, b: &Type) -> Type {
    if numeric_rank(a) >= numeric_rank(b) {
        a.clone()
    } else {
        b.clone()
    }
}

impl<'a> Analyzer<'a> {
    pub fn new(stmts: &'a Vec<Stmt>) -> Self {
        Self {
            stmts,
            errors: Vec::new(),
            scopes: vec![HashMap::new()], // start with global scope
            functions: HashMap::new(),
            structs: HashMap::new(),
            current_ret_type: Type::Unknown,
            loop_depth: 0,
        }
    }

    pub fn check_inits(&mut self) {
        for i in self.stmts.iter() {
            match i {
                Stmt::InitFunc {
                    name,
                    args,
                    ret_type,
                    data,
                } => {
                    let params: Vec<ArgData> = {
                        let mut res: Vec<ArgData> = Vec::new();
                        for i in args {
                            match i {
                                Stmt::Declaration(v) => {
                                    res.push(ArgData {
                                        arg_name: v.name.clone(),
                                        arg_type: v.ty.clone(),
                                    });
                                }
                                _ => panic!("smth"),
                            }
                        }
                        res
                    };
                    let func_data = SemFuncData {
                        args: params,
                        ret_type: ret_type.clone(),
                    };
                    self.functions.insert(name.clone(), func_data);
                }
                Stmt::InitStruct(data) => {
                    let mut element_size = 0;
                    let fields = {
                        let mut res: HashMap<String, StructField> = HashMap::new();
                        for i in data.fields.iter() {
                            res.insert(i.name.clone(), i.clone());
                            let el_size = type_size(&i.ty, &self.structs);
                            if element_size < el_size {
                                element_size = el_size;
                            }
                        }
                        res
                    };
                    let struct_data = StructData {
                        element_size,
                        elements: fields,
                    };
                    self.structs.insert(data.name.clone(), struct_data);
                }
                _ => {}
            }
        }
    }

    pub fn lookup(&mut self, expected_name: &String) -> Option<Type> {
        for i in self.scopes.iter() {
            for (name, ty) in i {
                if name == expected_name {
                    return Some(ty.clone());
                }
            }
        }
        return None;
    }

    pub fn add_var(&mut self, name: String, ty: Type) {
        let map = self.scopes.last_mut().unwrap();
        map.insert(name, ty);
    }

    pub fn check_code(&mut self) {
        //first iteration to get all structs and func data
        self.check_inits();
        // checking of every stmt
        for i in self.stmts.iter() {
            self.check_stmt(i);
        }
    }
}
