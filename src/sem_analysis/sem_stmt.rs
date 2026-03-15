use std::collections::HashMap;

use crate::{
    Gen::lvalue_root,
    Ir::{
        Stmt,
        expr::Expr,
        r#gen::{FuncData, StructData},
        sem_analysis::{Analyzer, ArgData, SemFuncData, SemanticError},
        stmt::{Declaration, LValue, StructDef, Type},
    },
    Tokenizer::TokenType,
};

impl<'a> Analyzer<'a> {
    pub fn check_block(&mut self, data: &Vec<Stmt>) {
        for i in data.iter() {
            self.check_stmt(i);
        }
    }

    pub fn check_types(&mut self, left: &Type, right: &Type) -> bool {
        // redo
        // add compatible_types so when we have
        // short and int
        // it wouldnt throw an error
        if left == right {
            return true;
        }
        false
    }

    pub fn check_declaration(&mut self, data: &Declaration) {
        if self.lookup(&data.name).is_some() {
            self.errors
                .push(SemanticError::AlreadyDeclared(data.name.clone()));
        }

        if let Some(expr) = &data.initializer {
            let expr_ty = self.check_expr(expr);
            if data.ty != expr_ty {
                self.errors.push(SemanticError::TypeMismatch {
                    expected: data.ty.clone(),
                    got: expr_ty,
                });
            }
        }

        self.add_var(data.name.clone(), data.ty.clone());
    }

    pub fn check_assignment(&mut self, target: &LValue, value: &Expr) {
        let expr_ty = self.check_expr(value);
        let var_name = lvalue_root(target);
        let var_data = self.lookup(&var_name);
        if var_data.is_none() {
            self.errors
                .push(SemanticError::UndeclaredVariable(var_name));
        } else if let Some(var_type) = &var_data {
            if !self.check_types(var_type, &expr_ty) {
                self.errors.push(SemanticError::TypeMismatch {
                    expected: var_type.clone(),
                    got: expr_ty,
                });
            }
        }
    }

    pub fn check_if(
        &mut self,
        condition: &Expr,
        if_block: &Box<Stmt>,
        else_block: &Option<Box<Stmt>>,
    ) {
        let _expr_ty = self.check_expr(condition);
        self.check_stmt(if_block);
        if let Some(else_data) = &else_block {
            self.check_stmt(else_data);
        }
    }

    pub fn check_while(&mut self, condition: &Expr, body: &Box<Stmt>) {
        let _expr_ty = self.check_expr(condition);
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
            self.check_expr(condition_data);
        }
        if let Some(update_data) = update {
            self.check_stmt(update_data);
        }
        self.check_stmt(body);
    }

    pub fn check_ret(&mut self, expr: &Option<Expr>) {
        let mut expr_ty = Type::Primitive(TokenType::Void);
        if let Some(expr) = expr {
            expr_ty = self.check_expr(expr);
        }
        if self.current_ret_type != expr_ty {
            self.errors.push(SemanticError::ReturnTypeMismatch {
                expected: self.current_ret_type.clone(),
                got: expr_ty.clone(),
            });
        }
    }

    pub fn check_init_func(&mut self, data: (&String, &Vec<Stmt>, &Type, &Box<Stmt>)) {
        let (name, args, ret_type, data) = data;
        if self.functions.get(name).is_none() {
            println!("something strange inside check_init_func");
        }
        let func_args = {
            let mut res: Vec<ArgData> = Vec::new();
            for i in args.iter() {
                match i {
                    Stmt::Declaration(v) => {
                        res.push(ArgData {
                            arg_name: v.name.clone(),
                            arg_type: v.ty.clone(),
                        });
                        self.add_var(v.name.clone(), v.ty.clone());
                    }
                    _ => panic!("smth"),
                }
            }
            res
        };
        let func_data = SemFuncData {
            args: func_args,
            ret_type: ret_type.clone(),
        };
        self.functions.insert(name.clone(), func_data);
        self.current_ret_type = ret_type.clone();
        self.check_stmt(data);
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
                elements,
                element_size: data.size,
            };

            self.structs.insert(data.name.clone(), struct_data);
        }
    }

    pub fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(data) => self.check_block(data),
            Stmt::Declaration(data) => self.check_declaration(data),
            Stmt::Assignment { target, value } => self.check_assignment(target, value),
            Stmt::ExprStmt(expr) => {
                self.check_expr(expr);
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
            } => {
                self.check_init_func((name, args, ret_type, data));
            }
            Stmt::InitStruct(struct_data) => self.check_struct_init(struct_data),
        }
    }
}
