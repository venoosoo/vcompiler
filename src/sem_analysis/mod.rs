use std::{collections::HashMap, env::var};

use crate::{
    Ir::{
        Stmt,
        r#gen::StructData,
        sem_analysis::*,
        stmt::{EnumData, StructField, Type},
    },
    tokenizer::TokenType,
};

pub mod sem_expr;
mod sem_stmt;

fn numeric_rank(ty: &Type) -> Option<u8> {
    match ty {
        //Type::Primitive(TokenType::Bool)  => Some(0),
        Type::Primitive(TokenType::CharType) => Some(1),
        Type::Primitive(TokenType::ShortType) => Some(2),
        Type::Primitive(TokenType::IntType) => Some(3),
        Type::Primitive(TokenType::LongType) => Some(4),
        //Type::Primitive(TokenType::Float) => Some(4),
        _ => None,
    }
}

pub fn is_numeric(ty: &Type) -> bool {
    numeric_rank(ty).is_some()
}

fn is_arithmetic(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(TokenType::CharType)
            | Type::Primitive(TokenType::IntType)
            | Type::Primitive(TokenType::LongType) //Type::Primitive(TokenType::Float)
    )
}

fn is_ptr_long_pair(a: &Type, b: &Type) -> bool {
    matches!(a, Type::Pointer(_)) && *b == Type::Primitive(TokenType::LongType)
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

pub fn coerce_numeric(a: &Type, b: &Type) -> Type {
    if numeric_rank(a) >= numeric_rank(b) {
        a.clone()
    } else {
        b.clone()
    }
}

pub fn check_types(left: &Type, right: &Type) -> bool {
    if left == right {
        return true;
    }
    if matches!(left, Type::GenericType(_)) || matches!(right, Type::GenericType(_)) {
        return true;
    }
    // only allow numeric coercion, no ptr<->long
    if numeric_rank(left).is_some() && numeric_rank(right).is_some() {
        return true;
    }
    // char array compatible with char*
    if let Type::Array(elem, _) = left {
        if **elem == Type::Primitive(TokenType::CharType)
            && *right == Type::Pointer(Box::new(Type::Primitive(TokenType::CharType)))
        {
            return true;
        }
    }
    // void* compatible with any pointer
    let is_void_ptr = |t: &Type| *t == Type::Pointer(Box::new(Type::Primitive(TokenType::Void)));
    if is_void_ptr(left) && matches!(right, Type::Pointer(_)) {
        return true;
    }
    if is_void_ptr(right) && matches!(left, Type::Pointer(_)) {
        return true;
    }
    // GenericInst compatible with same base type
    if let (Type::GenericType(l_name), Type::GenericType(r_name)) = (left, right) {
        return l_name == r_name;
    }
    false
}

impl<'a> Analyzer<'a> {
    pub fn new(stmts: &'a Vec<Stmt>) -> Self {
        Self {
            stmts,
            errors: Vec::new(),
            scopes: vec![HashMap::new()], // start with global scope
            functions: HashMap::new(),
            structs: HashMap::new(),
            global_vars: HashMap::new(),
            enums: HashMap::new(),
            current_ret_type: Type::Unknown,
            loop_depth: 0,
        }
    }
    // this is just copy from gen
    // TODO: make this a trait so and expand it for gen and analyzer
    pub fn type_size(&self, ty: &Type) -> usize {
        match ty {
            Type::Primitive(token) => match token {
                TokenType::CharType => 1,
                TokenType::ShortType => 2,
                TokenType::IntType => 4,
                TokenType::LongType => 8,
                _ => panic!("Unsupported primitive type: {:?}", token),
            },
            Type::Pointer(_) => 8,
            Type::Array(elem_type, count) => self.type_size(elem_type),
            Type::Struct(name) => {
                self.structs
                    .get(name)
                    .expect(&format!("Unknown struct: {}", name))
                    .byte_size
            }
            Type::GenericInst(..) => todo!(),
            Type::GenericType(_) => todo!(),
            Type::Enum(_) => 8,
            Type::Unknown => panic!("unkown type"),
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
                    generic_types,
                } => {
                    let params: Vec<ArgData> = {
                        let func_args: Vec<ArgData> = args
                            .iter()
                            .map(|decl| {
                                self.add_var(decl.name.clone(), decl.ty.clone());
                                ArgData {
                                    arg_name: decl.name.clone(),
                                    arg_type: decl.ty.clone(),
                                }
                            })
                            .collect();
                        func_args
                    };
                    let func_data = SemFuncData {
                        args: params,
                        ret_type: ret_type.clone(),
                    };
                    self.functions.insert(name.clone(), func_data);
                }
                Stmt::InitStruct(data) => {
                    let fields = {
                        let mut res: HashMap<String, StructField> = HashMap::new();
                        for i in data.fields.iter() {
                            res.insert(i.name.clone(), i.clone());
                        }
                        res
                    };
                    let struct_data = StructData {
                        name: data.name.clone(),
                        generic_type: data.generic_type.clone(),
                        byte_size: data.size,
                        elements: fields,
                    };
                    self.structs.insert(data.name.clone(), struct_data);
                }
                Stmt::InitEnum {
                    name,
                    variants,
                    generic_types,
                } => {
                    let enum_data = EnumData {
                        name: name.clone(),
                        generic_type: generic_types.clone(),
                        variants: variants.clone(),
                    };
                    self.enums.insert(name.clone(), enum_data);
                }
                _ => {}
            }
        }
    }

    pub fn lookup(&self, expected_name: &String) -> Option<Type> {
        for i in self.scopes.iter() {
            for (name, ty) in i {
                if name == expected_name {
                    return Some(ty.clone());
                }
            }
        }
        if let Some(global_data) = self.global_vars.get(expected_name) {
            return Some(global_data.clone());
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
