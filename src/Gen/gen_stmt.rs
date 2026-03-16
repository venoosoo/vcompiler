use std::alloc::Layout;
use std::env;
use std::fmt::format;
use std::fs::File;
use std::io::Read;

use super::*;

use crate::Ir::expr::Expr;
use crate::Ir::stmt::{LValue, StructDef};
use crate::Ir::{Stmt, stmt::Declaration};
use crate::Parser;
use crate::Tokenizer::Tokenizer;
impl Gen {
    fn gen_block(&mut self, data: &Vec<Stmt>) {
        self.scopes.push(HashMap::new());
        let temp_stack_pos = self.stack_pos;
        for i in data {
            self.gen_stmt(&i);
        }
        self.scopes.pop();
        self.stack_pos = temp_stack_pos;
    }

    fn gen_declaration(&mut self, data: &Declaration) {
        let stack_pos = self.alloc_type(&data.ty);

        if let Some(expr_data) = &data.initializer {
            let reg = self.eval_expr(expr_data, None, &data.ty);
            self.emit(format!("    mov [rbp - {}], {}", stack_pos, reg));
        }
        let current_scope = self.scopes.last_mut().unwrap();
        if current_scope.contains_key(&data.name) {
            self::panic!("Variable already declared in this scope");
        }
        let var_data = VarData {
            stack_pos,
            var_type: data.ty.clone(),
        };
        current_scope.insert(data.name.clone(), var_data);
    }

    pub fn calc_lvalue(&mut self, target: &LValue) -> (Addr, Type) {
        match target {
            LValue::Variable(name) => {
                let var = self.lookup_var(name);
                (Addr::Stack(var.stack_pos as isize), var.var_type.clone())
            }

            LValue::Field { base, name } => {
                let (addr, ty) = self.calc_lvalue(base);

                match ty {
                    Type::Struct(struct_name) => {
                        let layout = self
                            .structs
                            .get(&struct_name)
                            .expect("no struct with that name");

                        let field = layout.elements.get(name).expect("no such field in struct");
                        let field_type = field.ty.clone();

                        match addr {
                            Addr::Stack(pos) => {
                                // subtract offset for stack-down layout
                                (Addr::Stack(pos - field.offset as isize), field_type)
                            }

                            Addr::Reg(reg) => {
                                // subtract offset from register base address
                                self.emit(format!("    sub {}, {}", reg, field.offset));
                                (Addr::Reg(reg), field_type)
                            }
                        }
                    }

                    _ => self::panic!("field access on non-struct"),
                }
            }

            LValue::Deref(inner) => {
                let (addr, ty) = self.calc_lvalue(inner);
                match ty {
                    Type::Pointer(inner_ty) => {
                        // rsi will hold dereferenced pointer at runtime
                        self.emit(format!(
                            "    mov rsi, {}",
                            match addr {
                                Addr::Stack(pos) => format!("[rbp - {}]", pos),
                                Addr::Reg(reg) => reg.clone(),
                            }
                        ));
                        (Addr::Reg("rsi".to_string()), *inner_ty)
                    }
                    _ => self::panic!("dereference of non-pointer"),
                }
            }

            LValue::Index { base, index } => {
                let (addr, ty) = self.calc_lvalue(base);
                let index_reg = self.eval_expr(index, None, &ty); // evaluate index
                match &ty {
                    Type::Array(ty, size) => {
                        self.emit(format!("    cmp {}, {}", index_reg, size));
                        self.emit(format!("    jge __bounds_fail__"));
                        self.emit(format!("    cmp {}, 0", index_reg));
                        self.emit(format!("    jl __bounds_fail__"));
                    }
                    _ => {}
                }
                // assume element size is 4 bytes (adjust if needed)
                let elem_size = match ty {
                    Type::Primitive(TokenType::IntType) => 4,
                    Type::Primitive(TokenType::ShortType) => 2,
                    Type::Primitive(TokenType::CharType) => 1,
                    _ => 8, // for pointers / structs
                };
                self.emit(format!(
                    "    lea rax, [{} + {} * {}]",
                    match addr {
                        Addr::Stack(pos) => format!("rbp - {}", pos),
                        Addr::Reg(reg) => reg.clone(),
                    },
                    index_reg,
                    elem_size
                ));
                (Addr::Reg("rax".to_string()), ty)
            }
        }
    }

    fn gen_assignment(&mut self, target: &LValue, value: &Expr) {
        let (addr, ty) = self.calc_lvalue(target);

        let val_reg = self.eval_expr(value, None, &ty);

        match addr {
            Addr::Stack(pos) => {
                self.emit(format!("    mov [rbp - {}], {}", pos, val_reg));
            }

            Addr::Reg(reg) => {
                self.emit(format!("    mov [{}], {}", reg, val_reg));
            }
        }
    }

    pub fn gen_if(&mut self, data: (&Expr, &Box<Stmt>, &Option<Box<Stmt>>)) {
        let (condition, if_block, else_block) = data;

        let cond_reg = self.eval_expr(condition, None, &Type::Primitive(TokenType::LongType));
        self.emit(format!("    cmp {}, 0", cond_reg));

        let id = self.get_id();

        if let Some(else_stmt) = else_block {
            self.emit(format!("    je else_{}", id));
            self.emit(format!("if_{}:", id));
            self.gen_stmt(if_block);
            self.emit(format!("    jmp end_if_{}", id));
            self.emit(format!("else_{}:", id));
            self.gen_stmt(else_stmt);
        } else {
            self.emit(format!("    je end_if_{}", id));
            self.emit(format!("if_{}:", id));
            self.gen_stmt(if_block);
        }
        self.emit(format!("end_if_{}:", id));
    }

    pub fn gen_while(&mut self, data: (&Expr, &Box<Stmt>)) {
        let (condition, body) = data;
        let id = self.get_id();
        self.emit(format!("while_{}:", id));
        let cond_reg = self.eval_expr(condition, None, &Type::Primitive(TokenType::LongType));
        self.emit(format!("    cmp {}, 0", cond_reg));
        self.emit(format!("    jne end_while_{}", id));
        self.gen_stmt(&*body);
        self.emit(format!("    je while_{}", id));
        self.emit(format!("end_while_{}:", id));
    }

    pub fn gen_for(
        &mut self,
        data: (
            &Option<Box<Stmt>>,
            &Option<Expr>,
            &Option<Box<Stmt>>,
            &Box<Stmt>,
        ),
    ) {
        let (init, condition, update, body) = data;

        let id = self.get_id();
        self.scopes.push(HashMap::new());
        if let Some(init_stmt) = init {
            self.gen_stmt(init_stmt);
        }
        self.emit(format!("for_start_{}:", id));

        if let Some(cond_expr) = condition {
            let cond_reg = self.eval_expr(cond_expr, None, &Type::Primitive(TokenType::LongType));
            self.emit(format!("    cmp {}, 0", cond_reg));
            self.emit(format!("    je for_end_{}", id));
        }

        self.gen_stmt(&body);
        if let Some(update_stmt) = update {
            self.gen_stmt(update_stmt);
        }
        self.scopes.pop();
        self.emit(format!("    jmp for_start_{}", id));

        self.emit(format!("for_end_{}:", id));
    }

    fn gen_ret(&mut self, expr: &Option<Expr>) {
        if let Some(ret_expr) = expr {
            let ret_type = self.current_return_type.clone();

            // Evaluate using function return type
            let reg = self.eval_expr(ret_expr, None, &ret_type);

            // Move result into rax properly sized
            let sized_rax = reg_for_size("rax", &ret_type);

            if reg != sized_rax {
                self.emit(format!("    mov {}, {}", sized_rax, reg));
            }
        }

        self.emit("    mov rsp, rbp".to_string());
        self.emit("    pop rbp".to_string());
        self.emit("    ret".to_string());
    }

    pub fn gen_inline_asm(&mut self, data: &Vec<String>) {
        for i in data.iter() {
            let mut var_buf = String::new();
            let mut buf = String::new();
            let mut iter = i.chars();

            while let Some(j) = iter.next() {
                if j != '(' {
                    buf.push(j);
                } else {
                    while let Some(next) = iter.next() {
                        if next == ')' {
                            break;
                        } else {
                            var_buf.push(next);
                        }
                    }
                    let var = self.lookup_var(&var_buf);
                    buf.push_str(&format!("[rbp - {}]", var.stack_pos));
                }
            }
            self.emit(format!("    {}", buf));
        }
    }

    pub fn compile_args(&mut self, args: &Vec<Stmt>) {
        let arg_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9", "r10", "r11"];
        let mut pos = 0;
        for (i, stmt) in args.iter().enumerate() {
            let decl = match stmt {
                Stmt::Declaration(d) => d,
                _ => self::panic!("arg must be a declaration"),
            };

            if i >= arg_regs.len() {
                self::panic!("too many args, stack args not supported yet");
            }
            pos += self.type_size(&decl.ty);
            let reg = reg_for_size(arg_regs[i], &decl.ty);

            self.emit(format!("    mov [rbp - {}], {}", pos, reg));
        }
    }

    fn stmt_local_size(&self, stmt: &Stmt) -> usize {
        match stmt {
            Stmt::Declaration(decl) => self.type_size(&decl.ty),
            Stmt::For {
                init: Some(init), ..
            } => self.stmt_local_size(init),
            _ => 0,
        }
    }

    pub fn gen_func(&mut self, data: (&String, &Vec<Stmt>, &Type, &Box<Stmt>)) {
        let (name, args, ret_type, data) = data;
        self.current_return_type = ret_type.clone();
        let func_stack_frame = self.calc_stack_size(&data);
        self.emit(format!("{}:", name));
        self.emit(format!("    push rbp"));
        self.emit(format!("    mov rbp, rsp"));
        self.emit(format!("    sub rsp, {}", func_stack_frame));
        self.scopes.push(HashMap::new());
        let saved_stack = self.stack_pos;
        self.compile_args(args);
        for arg in args {
            self.gen_stmt(arg);
        }
        self.gen_stmt(data);
        self.scopes.pop();
        self.stack_pos = saved_stack;
        match ret_type {
            Type::Primitive(ty) => {
                if *ty == TokenType::Void {
                    self.emit(format!("    mov rsp, rbp"));
                    self.emit(format!("    pop rbp"));
                    self.emit(format!("    ret"));
                }
            }
            _ => {}
        }
    }

    pub fn gen_init_struct(&mut self, data: &StructDef) {
        let mut elements = HashMap::new();

        for field in &data.fields {
            elements.insert(field.name.clone(), field.clone());
        }

        let struct_data = StructData {
            elements,
            element_size: data.size,
        };

        self.structs.insert(data.name.clone(), struct_data);

        self.emit(format!("; ===== struct {} =====", data.name));
        self.emit(format!("; size: {}", data.size));

        for field in &data.fields {
            self.emit(format!(
                "; field {} | offset: {} | type: {:?}",
                field.name, field.offset, field.ty
            ));
        }

        self.emit(format!("; ======================"));
    }

    pub fn calc_stack_size(&self, body: &Stmt) -> usize {
        let mut max_depth = 0usize;
        self.calc_stack_recursive(body, 0, &mut max_depth);
        // Align to 16 bytes (System V ABI requirement)
        align16(max_depth)
    }

    pub fn type_size(&self, ty: &Type) -> usize {
        match ty {
            Type::Primitive(token) => match token {
                TokenType::CharType => 1,
                TokenType::ShortType => 2,
                TokenType::IntType => 4,
                TokenType::LongType => 8,
                _ => self::panic!("Unsupported primitive type: {:?}", token),
            },
            Type::Pointer(_) => 8,
            Type::Array(elem_type, count) => self.type_size(elem_type) * *count,
            Type::Struct(name) => {
                self.structs
                    .get(name)
                    .expect(&format!("Unknown struct: {}", name))
                    .element_size
                    * self.structs.get(name).unwrap().elements.len()
            }
            Type::Unknown => self::panic!("unkown type"),
        }
    }

    fn calc_stack_recursive(&self, stmt: &Stmt, current: usize, max_depth: &mut usize) {
        match stmt {
            Stmt::Block(stmts) => {
                let mut block_current = current;
                for s in stmts {
                    self.calc_stack_recursive(s, block_current, max_depth);
                    block_current += self.stmt_local_size(s);
                }
            }

            Stmt::Declaration(decl) => {
                let new_depth = current + self.type_size(&decl.ty);
                if new_depth > *max_depth {
                    *max_depth = new_depth;
                }
            }

            Stmt::If {
                condition: _,
                if_block,
                else_block,
            } => {
                self.calc_stack_recursive(if_block, current, max_depth);
                if let Some(else_b) = else_block {
                    self.calc_stack_recursive(else_b, current, max_depth);
                }
            }

            Stmt::While { condition: _, body } => {
                self.calc_stack_recursive(body, current, max_depth);
            }

            Stmt::For {
                init,
                condition: _,
                update,
                body,
            } => {
                let mut for_current = current;
                if let Some(init_stmt) = init {
                    self.calc_stack_recursive(init_stmt, for_current, max_depth);
                    for_current += self.stmt_local_size(init_stmt);
                }
                if let Some(update_stmt) = update {
                    self.calc_stack_recursive(update_stmt, for_current, max_depth);
                }
                self.calc_stack_recursive(body, for_current, max_depth);
            }

            // InitFunc: nested function definition — don't count its stack in ours
            Stmt::InitFunc { .. } => {}

            // These don't allocate stack space themselves
            Stmt::Assignment { .. }
            | Stmt::ExprStmt(_)
            | Stmt::Return(_)
            | Stmt::AsmCode(_)
            | Stmt::InitStruct(_) => {
                if current > *max_depth {
                    *max_depth = current;
                }
            }
            Stmt::Import(..) => {},
        }
    }

    fn gen_import(&mut self, file_name: &String) {
        let mut base_dir = env::current_dir().unwrap();
        base_dir.push(file_name);
        let file = File::open(base_dir);
        match file {
            Ok(mut file) => {
                if self.imported_files.contains(file_name) {
                    return;
                }
                let mut content = String::new();
                file.read_to_string(&mut content).unwrap();
                let mut tokenizer = Tokenizer::new(content);
                tokenizer.tokenize();
                
                let mut parser = Parser::Parser::new(tokenizer.m_res);
                let res = parser.parse();

                self.imported_files.insert(file_name.clone());

                self.reg_inits(&res);

                for i in &res {
                    self.gen_stmt(i);
                }
            }
            Err(_) => {}
        }
    }

    pub fn gen_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(v) => self.gen_block(v),
            Stmt::Declaration(v) => self.gen_declaration(v),
            Stmt::Assignment { target, value } => self.gen_assignment(target, value),
            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr, None, &Type::Primitive(TokenType::LongType));
            }
            Stmt::If {
                condition,
                if_block,
                else_block,
            } => {
                self.gen_if((condition, if_block, else_block));
            }
            Stmt::While { condition, body } => {
                self.gen_while((condition, body));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                self.gen_for((init, condition, update, body));
            }
            Stmt::Return(expr) => self.gen_ret(expr),
            Stmt::AsmCode(data) => self.gen_inline_asm(data),
            Stmt::InitFunc {
                name,
                args,
                ret_type,
                data,
            } => self.gen_func((name, args, ret_type, data)),
            Stmt::InitStruct(struct_data) => {} // skiping because we already added it in first iteration,
            Stmt::Import(file_name) => self.gen_import(file_name),
        }
    }
}
