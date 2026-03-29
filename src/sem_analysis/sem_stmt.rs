use std::{collections::HashMap, env, fs::File};

use super::*;

use crate::{
    Gen::lvalue_root,
    Ir::{
        Stmt,
        expr::Expr,
        r#gen::StructData,
        sem_analysis::{Analyzer, ArgData, SemFuncData, SemanticError},
        stmt::{Declaration, LValue, MatchField, MatchLeftValue, StructDef, Type},
    },
    Tokenizer::TokenType,
};

impl<'a> Analyzer<'a> {
    pub fn check_block(&mut self, data: &Vec<Stmt>) {
        self.scopes.push(HashMap::new());
        for i in data.iter() {
            self.check_stmt(i);
        }
        self.scopes.pop();
    }

    pub fn check_declaration(&mut self, data: &Declaration) {
        if data.ty == Type::Primitive(TokenType::Void) {
            self.errors
                .push(SemanticError::VoidVariable(data.name.clone()));
            return;
        }
        if self.lookup(&data.name).is_some() {
            self.errors
                .push(SemanticError::AlreadyDeclared(data.name.clone()));
        }

        if let Some(expr) = &data.initializer {
            let expr_ty = self.check_expr(expr, &data.ty);
            if !check_types(&data.ty, &expr_ty) {
                self.errors.push(SemanticError::TypeMismatch {
                    expected: data.ty.clone(),
                    got: expr_ty.clone(),
                });
            }
            if let (Type::Array(_, decl_size), Type::Array(_, init_size)) = (&data.ty, &expr_ty) {
                if init_size > decl_size {
                    self.errors.push(SemanticError::ArrayTooLarge {
                        arr_name: data.name.clone(),
                        expected: *decl_size,
                        got: *init_size,
                    });
                }
            }
        }

        self.add_var(data.name.clone(), data.ty.clone());
    }

    fn field_ty_match(&mut self, ty: &Type, name: &String) -> Type {
        match ty {
            Type::Pointer(p_ty) => {
                let res = self.field_ty_match(p_ty, name);
                res
            }

            Type::Struct(struct_name) => self
                .structs
                .get(struct_name)
                .and_then(|s| s.elements.get(name))
                .map(|f| f.ty.clone())
                .unwrap_or(Type::Unknown),
            _ => Type::Unknown,
        }
    }

    pub fn lvalue_type(&mut self, lvalue: &LValue) -> Type {
        match lvalue {
            LValue::Variable(name) => self.lookup(name).unwrap_or(Type::Unknown),
            LValue::Deref(inner) => {
                let inner_ty = self.lvalue_type(inner);
                match inner_ty {
                    Type::Pointer(ty) => *ty,
                    _ => Type::Unknown,
                }
            }
            LValue::Field { base, name } => {
                let base_ty = self.lvalue_type(base);
                self.field_ty_match(&base_ty, name)
            }
            LValue::Index { base, .. } => {
                let base_ty = self.lvalue_type(base);
                match base_ty {
                    Type::Array(elem_ty, _) => *elem_ty,
                    Type::Pointer(elem_ty) => *elem_ty,
                    _ => Type::Unknown,
                }
            }
        }
    }

    pub fn check_assignment(&mut self, target: &LValue, value: &Expr) {
        let var_name = lvalue_root(target);
        if self.lookup(&var_name).is_none() {
            self.errors
                .push(SemanticError::UndeclaredVariable(var_name));
            return;
        }

        // use lvalue_type for actual type check
        let target_ty = self.lvalue_type(target);
        let expr_ty = self.check_expr(value, &target_ty);
        if !check_types(&target_ty, &expr_ty) {
            self.errors.push(SemanticError::TypeMismatch {
                expected: target_ty,
                got: expr_ty,
            });
        }
    }

    pub fn check_if(
        &mut self,
        condition: &Expr,
        if_block: &Box<Stmt>,
        else_block: &Option<Box<Stmt>>,
    ) {
        let _expr_ty = self.check_expr(condition, &Type::Primitive(TokenType::LongType));
        self.check_stmt(if_block);
        if let Some(else_data) = &else_block {
            self.check_stmt(else_data);
        }
    }

    pub fn check_while(&mut self, condition: &Expr, body: &Box<Stmt>) {
        let _expr_ty = self.check_expr(condition, &Type::Primitive(TokenType::LongType));
        self.check_stmt(body);
    }

    pub fn check_for(
        &mut self,
        data: (
            &Option<Box<Stmt>>,
            &Option<Expr>,
            &Option<Box<Stmt>>,
            &Box<Stmt>,
        ),
    ) {
        let (init, condition, update, body) = data;
        if let Some(init_data) = init {
            self.check_stmt(init_data);
        }
        if let Some(condition_data) = condition {
            self.check_expr(condition_data, &Type::Primitive(TokenType::LongType));
        }
        if let Some(update_data) = update {
            self.check_stmt(update_data);
        }
        self.check_stmt(body);
    }

    pub fn check_ret(&mut self, expr: &Option<Expr>) {
        let mut expr_ty = Type::Primitive(TokenType::Void);
        if let Some(expr) = expr {
            expr_ty = self.check_expr(expr, &self.current_ret_type.clone());
        }
        if !check_types(&self.current_ret_type, &expr_ty) {
            self.errors.push(SemanticError::ReturnTypeMismatch {
                expected: self.current_ret_type.clone(),
                got: expr_ty.clone(),
            });
        }
    }

    pub fn check_init_func(
        &mut self,
        data: (&String, &Vec<Declaration>, &Type, &Box<Stmt>, &Vec<String>),
    ) {
        let (name, args, ret_type, body, generic_types) = data;

        if self.functions.get(name).is_none() {
            println!("something strange inside check_init_func");
        }

        // save outer scopes FIRST before adding any args
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);

        // push a fresh scope for function args and locals
        self.scopes.push(HashMap::new());

        let func_args: Vec<ArgData> = args
            .iter()
            .map(|decl| {
                self.add_var(decl.name.clone(), decl.ty.clone()); // now goes into function scope
                ArgData {
                    arg_name: decl.name.clone(),
                    arg_type: decl.ty.clone(),
                }
            })
            .collect();

        self.functions.insert(
            name.clone(),
            SemFuncData {
                args: func_args,
                ret_type: ret_type.clone(),
            },
        );

        self.current_ret_type = ret_type.clone();
        self.check_stmt(body);

        // restore outer scopes
        self.scopes = saved_scopes;
    }

    pub fn check_struct_init(&mut self, data: &StructDef) {
        if self.lookup(&data.name).is_some() {
            self.errors
                .push(SemanticError::AlreadyDeclared(data.name.clone()));
        } else {
            let mut elements = HashMap::new();
            for field in &data.fields {
                elements.insert(field.name.clone(), field.clone());
            }

            let struct_data = StructData {
                name: data.name.clone(),
                generic_type: Vec::new(),
                elements,
                byte_size: data.size,
            };

            self.structs.insert(data.name.clone(), struct_data);
        }
    }

    fn get_match_left_value_type(&self, lvalue: &MatchLeftValue) -> Type {
        match lvalue {
            MatchLeftValue::Enum { base, value, args } => {
                return Type::Enum(base.clone());
            }
            MatchLeftValue::Expr { expr } => expr.get_type(self),
        }
    }

    fn check_match(&mut self, expr: &Expr, variants: &Vec<MatchField>) {
        let expr_ty = expr.get_type(self);
        match expr_ty.clone() {
            Type::Enum(enum_data) => {
                for var in variants {
                    let left_ty = self.get_match_left_value_type(&var.left);

                    if !check_types(&left_ty, &expr_ty) {
                        match left_ty.clone() {
                            Type::Enum(name) => {
                                if name == "_" {
                                    continue;
                                }
                            }
                            _ => {}
                        }
                        self.errors.push(SemanticError::MatchTypeMismatch {
                            expected: expr_ty.clone(),
                            got: left_ty.clone(),
                        });
                    }
                }
            }
            Type::Primitive(ty) => {
                for var in variants {
                    let left_ty = self.get_match_left_value_type(&var.left);
                    if !check_types(&left_ty, &expr_ty) {
                        match left_ty.clone() {
                            Type::Enum(name) => {
                                if name == "_" {
                                    continue;
                                }
                            }
                            _ => {}
                        }
                        self.errors.push(SemanticError::MatchTypeMismatch {
                            expected: expr_ty.clone(),
                            got: left_ty.clone(),
                        });
                    }
                }
            }

            _ => {
                self.errors
                    .push(SemanticError::MatchExprUnsuported(expr_ty.clone()));
            }
        }
    }

    pub fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(data) => self.check_block(data),
            Stmt::Declaration(data) => self.check_declaration(data),
            Stmt::Assignment { target, value } => self.check_assignment(target, value),
            Stmt::ExprStmt(expr) => {
                self.check_expr(expr, &Type::Primitive(TokenType::LongType));
            }
            Stmt::If {
                condition,
                if_block,
                else_block,
            } => {
                self.check_if(condition, if_block, else_block);
            }
            Stmt::While { condition, body } => self.check_while(condition, body),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                self.check_for((init, condition, update, body));
            }
            Stmt::Return(expr) => self.check_ret(expr),
            Stmt::AsmCode(code) => {} // im not sure if there need for checking
            Stmt::InitFunc {
                name,
                args,
                ret_type,
                data,
                generic_types,
            } => {
                self.check_init_func((name, args, ret_type, data, generic_types));
            }
            Stmt::InitStruct(struct_data) => self.check_struct_init(struct_data),
            Stmt::GlobalDecl(global) => {
                if let Stmt::Declaration(decl) = global.as_ref() {
                    self.global_vars.insert(decl.name.clone(), decl.ty.clone());
                } else {
                    panic!("global decl must be a declaration");
                }
            }
            Stmt::InitEnum { .. } => {}
            Stmt::Match { expr, variants } => self.check_match(expr, variants),
        }
    }
}
