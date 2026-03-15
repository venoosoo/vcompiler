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
    fn check_num(&mut self, num: &i64) -> Type {
        return Type::Primitive(TokenType::IntType);
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

    fn check_binary(&mut self, op: &BinOp, left: &Box<Expr>, right: &Box<Expr>) -> Type {
        let l_type = self.check_expr(left);
        let r_type: Type = self.check_expr(right);
        let res = self.check_binary_types(op, l_type, r_type);
        match res {
            Ok(ty) => ty,
            Err(err) => {
                self.errors.push(err);
                Type::Unknown
            }
        }
    }

    fn check_unary(&mut self, op: &UnaryOp, expr: &Box<Expr>) -> Type {
        let expr_type = self.check_expr(expr);
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
                if !is_numeric(&l) || !is_numeric(&r) {
                    return Err(SemanticError::InvalidBinary {
                        op: op.clone(),
                        left: l,
                        right: r,
                    });
                }
                Ok(Type::Primitive(TokenType::IntType))
            }

            BinOp::Eq | BinOp::Neq => {
                let compatible = (is_numeric(&l) && is_numeric(&r)) || l == r;
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

    fn check_call(&mut self, name: &String, args: &Vec<Expr>) -> Type {
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
                let expr_ty = self.check_expr(expr);
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

    fn check_struct_member(&mut self, base: &Box<Expr>, name: &String) -> Type {
        let base = self.check_expr(base);
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

    fn check_deref(&mut self, expr: &Box<Expr>) -> Type {
        let expr_ty = self.check_expr(expr);
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

    fn check_addres_of(&mut self, expr: &Box<Expr>) -> Type {
        let expr_ty = self.check_expr(expr);
        return Type::Pointer(Box::new(expr_ty));
    }

    fn check_index(&mut self, base: &Box<Expr>, index: &Box<Expr>) -> Type {
        let base_ty = self.check_expr(base);
        let index_ty = self.check_expr(index);

        if !is_numeric(&index_ty) {
            self.errors
                .push(SemanticError::InvalidArrayIndex(index_ty.clone()));
        }

        match base_ty {
            Type::Array(arr_type, size) => *arr_type.clone(),
            Type::Pointer(ty) => todo!(),
            _ => {
                self.errors
                    .push(SemanticError::NonArrayIndex(base_ty.clone()));
                Type::Unknown
            }
        }
    }

    fn check_array_init(&mut self, elements: &Vec<Expr>) -> Type {
        if elements.is_empty() {
            self.errors.push(SemanticError::EmptyArray);
            return Type::Unknown;
        }

        let first_ty = self.check_expr(&elements[0]);

        for elem in elements.iter().skip(1) {
            let elem_ty = self.check_expr(elem);
            if !self.check_types(&first_ty, &elem_ty) {
                self.errors.push(SemanticError::TypeMismatch {
                    expected: first_ty.clone(),
                    got: elem_ty,
                });
            }
        }

        Type::Array(Box::new(first_ty), elements.len())
    }

    pub fn check_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            Expr::Number(num) => self.check_num(num),
            Expr::Float(num) => panic!("not implemented"),
            Expr::Variable(var) => self.check_var(var),
            Expr::Binary { op, left, right } => self.check_binary(op, left, right),
            Expr::Unary { op, expr } => self.check_unary(op, expr),
            Expr::Call { name, args } => self.check_call(name, args),
            Expr::StructInit {
                struct_name_ty,
                fields,
            } => self.check_struct_expr(struct_name_ty, fields),
            Expr::StructMember { base, name } => self.check_struct_member(base, name),
            Expr::Deref(expr) => self.check_deref(expr),
            Expr::AddressOf(expr) => self.check_addres_of(expr),
            Expr::Index { base, index } => self.check_index(base, index),
            Expr::ArrayInit { elements } => self.check_array_init(elements),
        }
    }
}
