use std::env::Args;

use crate::Ir::expr::{EnumExprField, Lookup};
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

impl<'a> Lookup for Analyzer<'a> {
    fn look_var(&self, name: &String) -> Type {
        if self.structs.get(name).is_some() {
            return Type::Struct(name.clone());
        } else {
            let var = self.lookup(name).unwrap();
            var
        }
    }
    fn look_unary(&self, op: &UnaryOp, expr: &Box<Expr>) -> Type {
        match op {
            UnaryOp::Neg => expr.get_type(self),
            UnaryOp::Not => Type::Primitive(TokenType::CharType), // boolean
        }
    }
    fn look_binary(&self, op: &BinOp, left: &Box<Expr>, right: &Box<Expr>) -> Type {
        let lty = left.get_type(self);
        let rty = right.get_type(self);
        coerce_numeric(&lty, &rty)
    }
    fn look_struct_init(&self, struct_name: &String) -> Type {
        if let Some(_struct_data) = self.structs.get(struct_name) {
            Type::Struct(struct_name.clone())
        } else {
            panic!("Struct {} not found in get_type", struct_name);
        }
    }
    fn look_deref(&self, ptr_expr: &Box<Expr>) -> Type {
        println!("expr: {:?}", ptr_expr);
        match ptr_expr.get_type(self) {
            Type::Pointer(inner) => *inner,
            _ => panic!("Cannot dereference a non-pointer"),
        }
    }
    fn look_addres_of(&self, var_expr: &Box<Expr>) -> Type {
        let ty = var_expr.get_type(self);
        Type::Pointer(Box::new(ty))
    }
    fn look_index(&self, base: &Box<Expr>, index: &Box<Expr>) -> Type {
        let base_ty = base.get_type(self);
        let idx_ty = index.get_type(self);
        if !is_numeric(&idx_ty) {
            panic!("Array index must be integer");
        }
        match base_ty {
            Type::Array(elem_ty, _) => *elem_ty,
            Type::Pointer(elem_ty) => *elem_ty,
            _ => panic!("Cannot index into non-array type"),
        }
    }
    fn look_struct_member(&self, base: &Box<Expr>, name: &String) -> Type {
        let base_ty = base.get_type(self);
        let struct_name = match &base_ty {
            Type::Struct(n) => n.clone(),
            Type::Pointer(inner) => match inner.as_ref() {
                Type::Struct(n) => n.clone(),
                _ => panic!("pointer to non-struct"),
            },
            _ => panic!("member access on non-struct"),
        };
        let struct_data = self.structs.get(&struct_name).unwrap();
        let field = struct_data.elements.get(name).unwrap();
        field.ty.clone()
    }
    fn look_call(&self, name: &String, args: &Vec<Expr>) -> Type {
        let vec_func_data = self.functions.get(name).unwrap();
        let func_data =
            vec_func_data.args.iter().enumerate().all(|(index, expr)| {
                check_types(&expr.arg_type, &vec_func_data.args[index].arg_type)
            });
        vec_func_data.ret_type.clone()
    }
    fn look_array_init(&self, elements: &Vec<Expr>) -> Type {
        if elements.len() > 0 {
            return elements[0].get_type(self);
        } else {
            Type::Unknown
        }
    }

    fn look_get_enum(&self, base: &String) -> Type {
        Type::Enum(base.clone())
    }
}

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
                if !check_types(&func_data.args[arg].arg_type, &expr_ty) {
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
                if check_types(&struct_arg.ty, &arg.get_type(self)) {
                    self.errors.push(SemanticError::StructTypeMismatch {
                        struct_name: struct_name.clone(),
                        expected: struct_arg.ty.clone(),
                        got: arg.get_type(self),
                    });
                }
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
            if !check_types(&first_ty, &elem_ty) {
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

    fn check_gen_enum(
        &mut self,
        base: &String,
        value: &Vec<EnumExprField>,
        variant: &String,
    ) -> Type {
        let enum_data = self
            .enums
            .get(base)
            .expect(&format!("no enums with name: {}\n{:?}", base, self.enums));
        let variant_data = enum_data
            .variants
            .get(variant)
            .expect(&format!("in struct: {} no variant {}", base, variant));
        for (index, arg) in variant_data.args.iter().enumerate() {
            let enum_field = &value[index];
            if !check_types(&enum_field.expr.get_type(self), &arg.ty) {
                panic!("debil²")
            }
        }
        Type::Enum(base.clone())
    }

    pub fn check_expr(&mut self, expr: &Expr, expected_ty: &Type) -> Type {
        match expr {
            Expr::Number(num) => self.check_num(num, expected_ty),
            Expr::Float(num) => panic!("not implemented"),
            Expr::Variable(var) => self.check_var(var),
            Expr::Binary { op, left, right } => self.check_binary(op, left, right, expected_ty),
            Expr::Unary { op, expr } => self.check_unary(op, expr, expected_ty),
            Expr::Call {
                name,
                args,
                generics,
            } => self.check_call(name, args, expected_ty),
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
            Expr::GetEnum {
                base,
                value,
                variant,
            } => self.check_gen_enum(base, value, variant),
            Expr::Cast { expr, ty } => ty.clone(),
        }
    }
}
