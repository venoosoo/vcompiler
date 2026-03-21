use std::alloc::Layout;
use std::env;
use std::fmt::format;
use std::fs::File;
use std::io::Read;

use super::*;

use crate::Ir::expr::Expr;
use crate::Ir::stmt::{LValue, StructDef};
use crate::Ir::{Stmt, stmt::Declaration};
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

        let current_scope = self.scopes.last_mut().unwrap();
        if current_scope.contains_key(&data.name) {
            self::panic!("Variable already declared in this scope");
        }

        let var_data = VarData {
            global_flag: false,
            stack_pos,
            var_type: data.ty.clone(),
        };
        current_scope.insert(data.name.clone(), var_data);

        if let Some(expr) = &data.initializer {
            self.eval_expr(expr, &data.ty);
            match &data.ty {
                Type::Primitive(_) | Type::Pointer(_) => {
                    let size_word = get_word(&data.ty);
                    let sized_reg = reg_for_size("rax", &data.ty).unwrap();
                    self.emit_main(format!(
                        "    mov {} [rbp - {}], {}",
                        size_word, stack_pos, sized_reg
                    ));
                }
                _ => {} // structs/arrays already written to stack by their eval_expr
            }
        }
    }

    pub fn calc_lvalue(&mut self, target: &LValue) -> (Addr, Type) {
        match target {
            LValue::Variable(name) => {
                let var = self.lookup_var(name);
                if !var.global_flag {
                    (Addr::Stack(var.stack_pos as isize), var.var_type.clone())
                } else {
                    (Addr::Reg(format!("rel {}", name)), var.var_type.clone())
                }
            }

            LValue::Field { base, name } => {
                let (addr, ty) = self.calc_lvalue(base);
                match ty {
                    Type::Pointer(inner) => match inner.as_ref() {
                        Type::Struct(struct_name) => {
                            // load pointer value into rsi, then deref
                            match &addr {
                                Addr::Stack(pos) => {
                                    self.emit_main(format!("    mov rsi, [rbp - {}]", pos));
                                }
                                Addr::Reg(reg) => {
                                    self.emit_main(format!("    mov rsi, [{}]", reg));
                                }
                            }

                            let field = self
                                .structs
                                .get(struct_name)
                                .unwrap()
                                .elements
                                .get(name)
                                .unwrap()
                                .clone();
                            self.emit_main(format!("    add rsi, {}", field.offset));
                            (Addr::Reg("rsi".to_string()), field.ty.clone())
                        }
                        _ => self::panic!("field access on non-struct pointer"),
                    },
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
                                self.emit_main(format!("    add {}, {}", reg, field.offset));
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
                        match &addr {
                            Addr::Stack(pos) => {
                                self.emit_main(format!("    mov rsi, [rbp - {}]", pos));
                            }
                            Addr::Reg(reg) => {
                                self.emit_main(format!("    mov rsi, [{}]", reg));
                            }
                        }
                        (Addr::Reg("rsi".to_string()), *inner_ty)
                    }
                    _ => self::panic!("deref of non-pointer"),
                }
            }
            LValue::Index { base, index } => {
                let (addr, ty) = self.calc_lvalue(base);
                let index_reg = self.eval_expr(index, &ty); // evaluate index
                match &ty {
                    Type::Array(ty, size) => {
                        self.emit_main(format!("    cmp {}, {}", index_reg, size));
                        self.emit_main(format!("    jge __bounds_fail__"));
                        self.emit_main(format!("    cmp {}, 0", index_reg));
                        self.emit_main(format!("    jl __bounds_fail__"));
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
                self.emit_main(format!(
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
        let val_reg = self.eval_expr(value, &ty);
        let sized_reg = reg_for_size("rax", &ty).unwrap();
        match addr {
            Addr::Stack(pos) => {
                let size_word = get_word(&ty);
                self.emit_main(format!(
                    "    mov {} [rbp - {}], {}",
                    size_word, pos, sized_reg
                ));
            }
            Addr::Reg(reg) => {
                let size_word = get_word(&ty);

                let sized_reg = reg_for_size(&val_reg, &ty).unwrap();
                self.emit_main(format!("    mov {} [{}], {}", size_word, reg, sized_reg));
            }
        }
    }

    pub fn gen_if(&mut self, data: (&Expr, &Box<Stmt>, &Option<Box<Stmt>>)) {
        let (condition, if_block, else_block) = data;

        self.eval_expr(condition, &Type::Primitive(TokenType::LongType));
        self.emit_main(format!("    cmp rax, 0"));

        let id = self.get_id();

        if let Some(else_stmt) = else_block {
            self.emit_main(format!("    je else_{}", id));
            self.emit_main(format!("if_{}:", id));
            self.gen_stmt(if_block);
            self.emit_main(format!("    jmp end_if_{}", id));
            self.emit_main(format!("else_{}:", id));
            self.gen_stmt(else_stmt);
        } else {
            self.emit_main(format!("    je end_if_{}", id));
            self.emit_main(format!("if_{}:", id));
            self.gen_stmt(if_block);
        }
        self.emit_main(format!("end_if_{}:", id));
    }

    pub fn gen_while(&mut self, data: (&Expr, &Box<Stmt>)) {
        let (condition, body) = data;
        let id = self.get_id();
        self.emit_main(format!("while_{}:", id));
        self.eval_expr(condition, &Type::Primitive(TokenType::LongType));
        self.emit_main(format!("    cmp rax, 0"));
        self.emit_main(format!("    je end_while_{}", id));
        self.gen_stmt(&*body);
        self.emit_main(format!("    jmp while_{}", id));
        self.emit_main(format!("end_while_{}:", id));
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
        self.emit_main(format!("for_start_{}:", id));

        if let Some(cond_expr) = condition {
            self.eval_expr(cond_expr, &Type::Primitive(TokenType::LongType));
            self.emit_main(format!("    cmp rax, 0"));
            self.emit_main(format!("    je for_end_{}", id));
        }

        self.gen_stmt(&body);
        if let Some(update_stmt) = update {
            self.gen_stmt(update_stmt);
        }
        self.scopes.pop();
        self.emit_main(format!("    jmp for_start_{}", id));

        self.emit_main(format!("for_end_{}:", id));
    }

    fn gen_ret(&mut self, expr: &Option<Expr>) {
        if let Some(ret_expr) = expr {
            let ret_type = self.current_return_type.clone();
            self.eval_expr(ret_expr, &ret_type); // result in rax/eax/ax/al
        }
        self.emit_main("    mov rsp, rbp".to_string());
        self.emit_main("    pop rbp".to_string());
        self.emit_main("    ret".to_string());
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
            self.emit_main(format!("    {}", buf));
        }
    }

    pub fn compile_args(&mut self, args: &Vec<Declaration>) {
        let arg_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        for (i, decl) in args.iter().enumerate() {
            if i >= arg_regs.len() {
                self::panic!("too many args, stack args not supported yet");
            }
            let pos = self.alloc_type(&decl.ty);
            let reg = reg_for_size(arg_regs[i], &decl.ty).unwrap();
            self.emit_main(format!("    mov [rbp - {}], {}", pos, reg));
            let map = self.scopes.last_mut().unwrap();
            map.insert(
                decl.name.clone(),
                VarData {
                    global_flag: false,
                    stack_pos: pos,
                    var_type: decl.ty.clone(),
                },
            );
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

    pub fn member_addr(&mut self, base: &Expr, field_name: &str) -> Type {
        let base_type = base.get_type_of_expr(self);
        self.eval_expr(base, &base_type); // rax = pointer to struct

        let struct_name = match &base_type {
            Type::Pointer(inner) => match inner.as_ref() {
                Type::Struct(name) => name.clone(),
                _ => self::panic!("pointer to non-struct"),
            },
            _ => self::panic!("-> on non-pointer, use . instead"),
        };

        let struct_data = self.structs.get(&struct_name).unwrap().clone();
        let field = struct_data.elements.get(field_name).unwrap();
        self.emit_main(format!("    add rax, {}", field.offset));
        field.ty.clone()
    }

    pub fn gen_func(&mut self, data: (&String, &Vec<Declaration>, &Type, &Box<Stmt>)) {
        let (name, args, ret_type, body) = data;
        self.current_return_type = ret_type.clone();

        let body_size = self.calc_stack_size(body);
        let arg_size = args.iter().map(|a| self.type_size(&a.ty)).sum::<usize>();
        let func_stack_frame = align16(body_size + arg_size);

        // save outer scopes, start fresh with globals only
        let global_scope = self.scopes[0].clone();
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![global_scope]);
        let saved_stack = self.stack_pos;

        self.emit_main(format!("{}:", name));
        self.emit_main("    push rbp".to_string());
        self.emit_main("    mov rbp, rsp".to_string());
        self.emit_main(format!("    sub rsp, {}", func_stack_frame));

        self.compile_args(args);
        self.gen_stmt(body);

        // restore outer scopes
        self.scopes = saved_scopes;
        self.stack_pos = saved_stack;

        match ret_type {
            Type::Primitive(ty) if *ty == TokenType::Void => {
                self.emit_main("    mov rsp, rbp".to_string());
                self.emit_main("    pop rbp".to_string());
                self.emit_main("    ret".to_string());
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
            byte_size: data.size,
        };

        self.structs.insert(data.name.clone(), struct_data);

        self.emit_main(format!("; ===== struct {} =====", data.name));
        self.emit_main(format!("; size: {}", data.size));

        for field in &data.fields {
            self.emit_main(format!(
                "; field {} | offset: {} | type: {:?}",
                field.name, field.offset, field.ty
            ));
        }

        self.emit_main(format!("; ======================"));
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
                    .byte_size
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
            Stmt::GlobalDecl(global) => self.calc_stack_recursive(&*global, current, max_depth),
        }
    }

    fn type_to_data_directive(&self, ty: &Type) -> &str {
        match self.type_size(ty) {
            8 => "dq",
            4 => "dd",
            2 => "dw",
            1 => "db",
            _ => "dq", // default to 8 for unknown/structs/arrays
        }
    }

    fn gen_global(&mut self, global: Box<Stmt>) {
        match *global {
            Stmt::Declaration(decl_data) => {
                if let Some(_) = decl_data.initializer {
                    self::panic!("global cant have expr");
                }
                self.emit_data(format!(
                    "{} {} 0",
                    decl_data.name,
                    self.type_to_data_directive(&decl_data.ty)
                ));
                let global_var_data = VarData {
                    global_flag: true,
                    stack_pos: 0,
                    var_type: decl_data.ty.clone(),
                };

                self.global_vars
                    .insert(decl_data.name.clone(), global_var_data);
                if let Some(expr_data) = &decl_data.initializer {
                    self.eval_expr(expr_data, &decl_data.ty);
                    match decl_data.ty {
                        Type::Primitive(_) | Type::Pointer(_) => {
                            self.emit_main(format!("    mov [rel {}], rax", decl_data.name));
                        }
                        _ => {}
                    }
                }
            }
            _ => self::panic!("trying to make global of strange stmt"),
        }
    }

    pub fn gen_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(v) => self.gen_block(v),
            Stmt::Declaration(v) => self.gen_declaration(v),
            Stmt::Assignment { target, value } => self.gen_assignment(target, value),
            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr, &Type::Primitive(TokenType::LongType));
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
            Stmt::GlobalDecl(global) => self.gen_global(global.clone()),
        }
    }
}
