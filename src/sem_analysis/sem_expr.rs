use std::env::Args;

use crate::Ir::sem_analysis::SemanticError;
use crate::{
    Ir::{
        expr::{BinOp, Expr, UnaryOp},
        sem_analysis::Analyzer,
        stmt::Type,
    },
    Tokenizer::TokenType,
};

use super::*;

impl<'a> Analyzer<'a> {
    fn check_num(&mut self, num: &i64, expected_ty: &Type) -> Type {
        match expected_ty {
            Type::Primitive(_) => expected_ty.clone(),
            _ => Type::Primitive(TokenType::IntType),
        }
    }
    fn check_var(&mut self, var: &String) -> Type {
        let var_data = self.lookup(var);
        if let Some(var) = var_data {
            var
        } else {
            self.errors
                .push(SemanticError::UndeclaredVariable(var.clone()));

            // satisfy return type it wouldnt be compiled because of error anyway
            Type::Primitive(TokenType::LongType)
        }
    }

    fn check_binary(
        &mut self,
        op: &BinOp,
        left: &Box<Expr>,
        right: &Box<Expr>,
        expected_ty: &Type,
    ) -> Type {
        let l_type = self.check_expr(left, expected_ty);
        let r_type: Type = self.check_expr(right, expected_ty);
        let res = self.check_binary_types(op, l_type, r_type);
        match res {
            Ok(ty) => ty,
            Err(err) => {
                self.errors.push(err);
                Type::Unknown
            }
        }
    }

    fn check_unary(&mut self, op: &UnaryOp, expr: &Box<Expr>, expected_ty: &Type) -> Type {
        let expr_type = self.check_expr(expr, expected_ty);
        let valid = match op {
            UnaryOp::Neg => is_arithmetic(&expr_type), // -int, -long, -float ok; -char not
            UnaryOp::Not => is_numeric(&expr_type),    // !int, !long etc (C-style, no bool yet)
        };
        if !valid {
            self.errors.push(SemanticError::InvalidUnary {
                op: op.clone(),
                ty: expr_type.clone(),
            });
            return Type::Unknown;
        }

        expr_type
    }

    pub fn check_binary_types(
        &mut self,
        op: &BinOp,
        l: Type,
        r: Type,
    ) -> Result<Type, SemanticError> {
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                if matches!(&l, Type::Pointer(_)) && is_integer(&r) {
                    return Ok(l);
                }
                if matches!(&r, Type::Pointer(_)) && is_integer(&l) {
                    return Ok(r);
                }
                if !is_arithmetic(&l) || !is_arithmetic(&r) {
                    return Err(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    });
                }
                Ok(coerce_numeric(&l, &r))
            }

            BinOp::Mod => {
                if !is_integer(&l) || !is_integer(&r) {
                    return Err(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    });
                }
                Ok(coerce_numeric(&l, &r))
            }

            BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                let compatible = (is_numeric(&l) && is_numeric(&r))
                    || is_ptr_long_pair(&l, &r)
                    || is_ptr_long_pair(&r, &l);
                if !compatible {
                    return Err(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    });
                }
                Ok(Type::Primitive(TokenType::IntType))
            }

            BinOp::Eq | BinOp::Neq => {
                let compatible = (is_numeric(&l) && is_numeric(&r))
                    || l == r
                    || is_ptr_long_pair(&l, &r)
                    || is_ptr_long_pair(&r, &l)
                    || matches!((&l, &r), (Type::Pointer(_), Type::Pointer(_)));
                if !compatible {
                    return Err(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    });
                }
                Ok(Type::Primitive(TokenType::IntType))
            }

            BinOp::And | BinOp::Or => {
                if !is_numeric(&l) || !is_numeric(&r) {
                    // any nonzero int is truthy
                    return Err(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    });
                }
                Ok(Type::Primitive(TokenType::IntType))
            }
        }
    }

    fn check_call(&mut self, name: &String, args: &Vec<Expr>, expected_ty: &Type) -> Type {
        let res = self.functions.get(name).cloned();
        if let Some(func_data) = res {
            if func_data.args.len() != args.len() {
                self.errors.push(SemanticError::ArgCountMismatch {
                    func: name.clone(),
                    expected: func_data.args.len(),
                    got: args.len(),
                });
                return Type::Unknown;
            }

            let error_len = self.errors.len();

            for (arg, expr) in args.iter().enumerate() {
                let expr_ty = self.check_expr(expr, expected_ty);
                if !self.check_types(&func_data.args[arg].arg_type, &expr_ty) {
                    self.errors.push(SemanticError::ArgTypeMismatch {
                        func: name.clone(),
                        pos: arg,
                        expected: func_data.args[arg].arg_type.clone(),
                        got: expr_ty,
                    });
                }
            }

            if error_len != self.errors.len() {
                return Type::Unknown;
            }

            func_data.ret_type.clone()
        } else {
            self.errors
                .push(SemanticError::UndeclaredFunction(name.clone()));
            Type::Unknown
        }
    }

    fn check_struct_expr(&mut self, struct_name: &String, fields: &Vec<(String, Expr)>) -> Type {
        let struct_data = self
            .structs
            .get(struct_name)
            .expect(&format!("no struct with name: {}", struct_name));
        if fields.len() != struct_data.elements.len() {
            self.errors.push(SemanticError::StructCountMismatch {
                struct_name: struct_name.clone(),
                expected: struct_data.elements.len(),
                got: fields.len(),
            });
        }

        for (arg_name, arg) in fields.iter() {
            let res = struct_data.elements.get(arg_name);
            if let Some(struct_arg) = res {
                // needs rework of get_type_of_expr
                //if struct_arg.ty != arg.get_type_of_expr(gen_helper) {
                //    self.errors.push(SemanticError::StructTypeMismatch { struct_name: struct_name.clone(), expected: struct_arg.ty.clone(), got: arg.get_type_of_expr(gen_helper) });
                //}
            } else {
                self.errors.push(SemanticError::StructNameNotFound {
                    struct_name: struct_name.clone(),
                    got: arg_name.clone(),
                });
            }
        }
        Type::Struct(struct_name.to_string())
    }

    fn check_struct_member(&mut self, base: &Box<Expr>, name: &String, expected_ty: &Type) -> Type {
        let base = self.check_expr(base, expected_ty);
        match base {
            Type::Struct(struct_name) => {
                let res = self.structs.get(&struct_name);
                if let Some(struct_data) = res {
                    let name_res = struct_data.elements.get(name);
                    if let Some(arg) = name_res {
                        return arg.ty.clone();
                    } else {
                        self.errors.push(SemanticError::StructNameNotFound {
                            struct_name,
                            got: name.clone(),
                        });
                        return Type::Unknown;
                    }
                } else {
                    self.errors
                        .push(SemanticError::UndeclaredStruct(struct_name));
                    return Type::Unknown;
                }
            }
            _ => {
                self.errors.push(SemanticError::NotAStruct(base.clone()));
                return Type::Unknown;
            }
        }
    }

    fn check_deref(&mut self, expr: &Box<Expr>, expected_ty: &Type) -> Type {
        let expr_ty = self.check_expr(expr, expected_ty);
        match expr_ty {
            Type::Pointer(ty) => {
                return *ty.clone();
            }
            _ => {
                self.errors.push(SemanticError::NotAPointer(expr_ty));
                Type::Unknown
            }
        }
    }

    fn check_addres_of(&mut self, expr: &Box<Expr>, expected_ty: &Type) -> Type {
        let expr_ty = self.check_expr(expr, expected_ty);
        return Type::Pointer(Box::new(expr_ty));
    }

    fn check_index(&mut self, base: &Box<Expr>, index: &Box<Expr>, expected_ty: &Type) -> Type {
        let base_ty = self.check_expr(base, expected_ty);
        let index_ty = self.check_expr(index, expected_ty);

        if !is_numeric(&index_ty) {
            self.errors
                .push(SemanticError::InvalidArrayIndex(index_ty.clone()));
        }

        match base_ty {
            Type::Array(arr_type, size) => *arr_type.clone(),
            Type::Pointer(ty) => *ty,
            _ => {
                self.errors
                    .push(SemanticError::NonArrayIndex(base_ty.clone()));
                Type::Unknown
            }
        }
    }

    fn check_array_init(&mut self, elements: &Vec<Expr>, expected_ty: &Type) -> Type {
        if elements.is_empty() {
            self.errors.push(SemanticError::EmptyArray);
            return Type::Unknown;
        }

        let first_ty = expected_ty;

        for elem in elements.iter().skip(1) {
            let elem_ty = self.check_expr(elem, expected_ty);
            if !self.check_types(&first_ty, &elem_ty) {
                self.errors.push(SemanticError::TypeMismatch {
                    expected: first_ty.clone(),
                    got: elem_ty,
                });
            }
        }

        Type::Array(Box::new(first_ty.clone()), elements.len())
    }

    fn check_size_of(&mut self, expr: &Stmt) -> Type {
        Type::Primitive(TokenType::LongType)
    }

    pub fn check_expr(&mut self, expr: &Expr, expected_ty: &Type) -> Type {
        match expr {
            Expr::Number(num) => self.check_num(num, expected_ty),
            Expr::Float(num) => panic!("not implemented"),
            Expr::Variable(var) => self.check_var(var),
            Expr::Binary { op, left, right } => self.check_binary(op, left, right, expected_ty),
            Expr::Unary { op, expr } => self.check_unary(op, expr, expected_ty),
            Expr::Call { name, args } => self.check_call(name, args, expected_ty),
            Expr::StructInit {
                struct_name_ty,
                fields,
            } => self.check_struct_expr(struct_name_ty, fields),
            Expr::StructMember { base, name } => self.check_struct_member(base, name, expected_ty),
            Expr::Deref(expr) => self.check_deref(expr, expected_ty),
            Expr::AddressOf(expr) => self.check_addres_of(expr, expected_ty),
            Expr::Index { base, index } => self.check_index(base, index, expected_ty),
            Expr::ArrayInit { elements } => self.check_array_init(elements, expected_ty),
            Expr::SizeOf { ty } => self.check_size_of(ty),
            Expr::String { str } => {
                return Type::Array(
                    Box::new(Type::Primitive(TokenType::CharType)),
                    str.len() + 1,
                );
            }
        }
    }
}
