use std::dbg;

use indexmap::IndexMap;

use super::*;

use crate::Ir::expr::{Expr, ExprType};
use crate::Ir::stmt::{LValue, MatchField, MatchLeftValue, StructDef, StructField};
use crate::Ir::{Stmt, stmt::Declaration};
use crate::shared::align16;

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
        let data_ty = self.generic_to_ty(&data.ty, &self.generics.borrow());
        let data_ty = self.ensure_monomorphized(&data_ty);

        let data_ty = match data_ty {
            Type::GenericType(name) => {
                let map = self.generics.borrow();

                map.get(&name).cloned().unwrap()
            }
            _ => data_ty,
        };
        let stack_pos = self.alloc_type(&data_ty);
        if let Some(expr) = &data.initializer {
            self.eval_expr(expr, &data_ty);
            match data_ty.clone() {
                Type::Primitive(_) | Type::Pointer(_) => {
                    let size_word = self.get_word(&data_ty);
                    let sized_reg = self.reg_for_size("rax", &data_ty).unwrap();
                    self.emit_func_data(format!(
                        "    mov {} [rbp - {}], {}",
                        size_word, stack_pos, sized_reg
                    ));
                }
                Type::Array(ref ty, size) => match **ty {
                    Type::Primitive(TokenType::U8) => {
                        let size_word = self.get_word(&data_ty);
                        let sized_reg = self.reg_for_size("rax", &data_ty).unwrap();
                        self.emit_func_data(format!(
                            "    mov {} [rbp - {}], {}",
                            size_word, stack_pos, sized_reg
                        ));
                    }
                    _ => {}
                },
                Type::Enum(name, variant) => match &expr.ty {
                    ExprType::GetEnum {
                        base,
                        variant,
                        value,
                    } => {
                        if value.len() == 0 {
                            let size_word = self.get_word(&data_ty);
                            let sized_reg = self.reg_for_size("rax", &data_ty).unwrap();
                            self.emit_func_data(format!(
                                "    mov {} [rbp - {}], {}",
                                size_word, stack_pos, sized_reg
                            ));
                        }
                    }
                    _ => {}
                },
                _ => {} // structs/arrays already written to stack by their eval_expr
            }
        }

        let current_scope = self.scopes.last_mut().unwrap();
        if current_scope.contains_key(&data.name) {
            self::panic!("Variable {} is already defined", data.name);
        }

        let var_data = VarData {
            global_flag: false,
            stack_pos,
            var_type: data_ty.clone(),
        };
        current_scope.insert(data.name.clone(), var_data);
    }

    pub fn calc_lvalue(&mut self, target: &LValue) -> (Addr, Type) {
        match target {
            LValue::Variable(name) => {
                let var = self.lookup_var(name);
                if !var.global_flag {
                    (Addr::Stack(var.stack_pos as isize), var.var_type.clone())
                } else {
                    (Addr::Reg(format!("{}", name)), var.var_type.clone())
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
                                    self.emit_func_data(format!("    mov rsi, [rbp - {}]", pos));
                                }
                                Addr::Reg(reg) => {
                                    self.emit_func_data(format!("    mov rsi, [{}]", reg));
                                }
                            }

                            let field = self
                                .structs
                                .borrow()
                                .get(struct_name)
                                .unwrap()
                                .elements
                                .get(name)
                                .unwrap()
                                .clone();
                            self.emit_func_data(format!("    add rsi, {}", field.offset));
                            (Addr::Reg("rsi".to_string()), field.ty.clone())
                        }
                        _ => self::panic!("field access on non-struct pointer"),
                    },
                    Type::Struct(struct_name) => {
                        let layout = self
                            .structs
                            .borrow()
                            .get(&struct_name)
                            .cloned()
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
                                self.emit_func_data(format!("    add {}, {}", reg, field.offset));
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
                                self.emit_func_data(format!("    mov rsi, [rbp - {}]", pos));
                            }
                            Addr::Reg(reg) => {
                                self.emit_func_data(format!("    mov rsi, [{}]", reg));
                            }
                        }
                        (Addr::Reg("rsi".to_string()), *inner_ty)
                    }
                    _ => self::panic!("deref of non-pointer"),
                }
            }
            LValue::Index { base, index } => {
                let (addr, ty) = self.calc_lvalue(base);
                let mut inner_ty = self.ensure_monomorphized(&ty);
                self.emit_func_data(format!("    push rax")); // save expr
                let mut index_reg = self.eval_expr(index, &ty); // evaluate index
                match &ty {
                    Type::Pointer(ty) | Type::Array(ty, _) => {
                        if index_reg == "al" {
                            index_reg = "rax".to_string()
                        };
                        self.emit_func_data(format!(
                            "    imul {}, {}",
                            index_reg,
                            self.type_size(&ty)
                        ));
                        inner_ty = *ty.clone();
                    }
                    _ => {}
                }

                match addr {
                    Addr::Reg(reg) => {
                        if matches!(ty, Type::Pointer(_)) {
                            self.emit_func_data(format!("    mov rcx, [{}]", reg));
                        } else {
                            self.emit_func_data(format!("    mov rcx, {}", reg));
                        }
                    }
                    Addr::Stack(pos) => {
                        if matches!(ty, Type::Pointer(_)) {
                            self.emit_func_data(format!("    mov rcx, [rbp - {}]", pos));
                        } else {
                            self.emit_func_data(format!("    lea rcx, [rbp - {}]", pos));
                        }
                    }
                }

                self.emit_func_data(format!("    add rcx, {}", index_reg));
                self.emit_func_data(format!("    pop rax"));
                (Addr::Reg("rcx".to_string()), inner_ty)
            }
        }
    }

    fn gen_assignment(&mut self, target: &LValue, value: &Expr) {
        let value_expr = value.get_type(self);
        let val_reg = self.eval_expr(value, &value_expr);
        let (addr, lval) = self.calc_lvalue(target);
        let size_word = self.get_word(&lval);
        match addr {
            Addr::Stack(pos) => {
                let sized_reg = self.reg_for_size("rax", &lval).unwrap();
                let sized_word = self.get_word(&lval);
                self.emit_func_data(format!(
                    "    mov {} [rbp - {}], {}",
                    sized_word, pos, sized_reg
                ));
            }
            Addr::Reg(reg) => {
                let sized_reg = self.reg_for_size(&val_reg, &lval).unwrap();
                self.emit_func_data(format!("    mov {} [{}], {}", size_word, reg, sized_reg));
            }
        }
    }

    pub fn gen_if(&mut self, data: (&Expr, &Box<Stmt>, &Option<Box<Stmt>>)) {
        let (condition, if_block, else_block) = data;

        self.eval_expr(condition, &Type::Primitive(TokenType::I64));
        self.emit_func_data(format!("    cmp rax, 0"));

        let id = self.get_id();

        if let Some(else_stmt) = else_block {
            self.emit_func_data(format!("    je else_{}", id));
            self.emit_func_data(format!("if_{}:", id));
            self.gen_stmt(if_block);
            self.emit_func_data(format!("    jmp end_if_{}", id));
            self.emit_func_data(format!("else_{}:", id));
            self.gen_stmt(else_stmt);
        } else {
            self.emit_func_data(format!("    je end_if_{}", id));
            self.emit_func_data(format!("if_{}:", id));
            self.gen_stmt(if_block);
        }
        self.emit_func_data(format!("end_if_{}:", id));
    }

    pub fn gen_while(&mut self, data: (&Expr, &Box<Stmt>)) {
        let (condition, body) = data;
        let id = self.get_id();
        self.emit_func_data(format!("while_{}:", id));
        self.eval_expr(condition, &Type::Primitive(TokenType::I64));
        self.emit_func_data(format!("    cmp rax, 0"));
        self.emit_func_data(format!("    je end_while_{}", id));
        self.gen_stmt(&*body);
        self.emit_func_data(format!("    jmp while_{}", id));
        self.emit_func_data(format!("end_while_{}:", id));
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
        self.emit_func_data(format!("for_start_{}:", id));

        if let Some(cond_expr) = condition {
            self.eval_expr(cond_expr, &Type::Primitive(TokenType::I64));
            self.emit_func_data(format!("    cmp rax, 0"));
            self.emit_func_data(format!("    je for_end_{}", id));
        }
        self.break_stack.push(format!("for_end_{}", id));
        self.contniue_stack.push(format!("for_start_{}", id));
        self.gen_stmt(&body);
        if let Some(update_stmt) = update {
            self.gen_stmt(update_stmt);
        }
        self.scopes.pop();
        self.break_stack.pop();
        self.contniue_stack.pop();
        self.emit_func_data(format!("    jmp for_start_{}", id));

        self.emit_func_data(format!("for_end_{}:", id));
    }

    fn copy_chunks_to_hidden_ret(&mut self, total_size: usize) {
        self.emit_func_data("    mov rdi, [rbp - 8]".to_string());
        self.emit_func_data("    mov rcx, rax".to_string());

        let chunks = (total_size + 7) / 8;
        for i in 0..chunks {
            let offset = i * 8;
            self.emit_func_data(format!("    mov rdx, [rcx + {}]", offset));
            self.emit_func_data(format!("    mov [rdi + {}], rdx", offset));
        }
        self.emit_func_data("    mov rax, [rbp - 8]".to_string());
    }

    fn gen_ret(&mut self, expr: &Option<Expr>) {
        if let Some(ret_expr) = expr {
            let ret_type = self.current_return_type.clone();
            let ret_type = self.ensure_monomorphized(&ret_type);
            match &ret_expr.ty {
                ExprType::Variable(name) => {
                    self.gen_expr_var_addr(name, &ret_type);
                }
                ExprType::GetEnum {
                    base,
                    variant,
                    value,
                } => {
                    self.alloc(self.type_size(&ret_type));
                    self.gen_get_enum_addr(base, value, variant,&ret_type);
                }
                ExprType::StructInit {
                    struct_name_ty,
                    fields,
                } => {
                    self.alloc(self.type_size(&ret_type));
                    self.eval_expr(ret_expr, &ret_type);
                }
                _ => {
                    self.alloc_type(&ret_type);
                    self.eval_expr(ret_expr, &ret_type);
                }
            }
            match &ret_type {
                Type::Struct(name) => {
                    let struct_data = self.structs.borrow().get(name).unwrap().clone();
                    self.copy_chunks_to_hidden_ret(struct_data.size);
                }
                Type::Enum(name, _) => {
                    let enum_data = self.enums.borrow().get(name).unwrap().clone();

                    match &ret_expr.ty {
                        ExprType::GetEnum {
                            base,
                            variant,
                            value,
                        } => {
                            self.copy_chunks_to_hidden_ret(enum_data.size);
                        }

                        ExprType::Call { .. } => match &ret_type {
                            Type::Enum(name, _) => {
                                let size = self.enums.borrow().get(name).unwrap().size;
                                self.alloc(size);
                                self.copy_chunks_to_hidden_ret(size);
                            }
                            Type::Struct(name) => {
                                let size = self.structs.borrow().get(name).unwrap().size;
                                self.alloc(size);
                                self.copy_chunks_to_hidden_ret(size);
                            }
                            _ => {}
                        },

                        _ => {
                            self.copy_chunks_to_hidden_ret(enum_data.size);
                        }
                    }
                }
                _ => {}
            }
        }
        self.emit_func_data("    mov rsp, rbp".to_string());
        self.emit_func_data("    pop rbp".to_string());
        self.emit_func_data("    ret".to_string());
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
            self.emit_func_data(format!("    {}", buf));
        }
    }

    fn get_arg(
        &mut self,
        pos: usize,
        ty: &Type,
        local_pos: usize,
        stack_arg_pos: Option<usize>,
        is_rvo: bool,
    ) {
        let arg_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        if pos > 6 {
            let size = self.type_size(ty);
            let stack_pos =
                stack_arg_pos.expect("the stack arg hasnt been provided the stack_arg_pos");
            let chunks = (size + 7) / 8;
            let remainder = size % 8;
            let full = if remainder > 0 { chunks - 1 } else { chunks };

            for i in 0..full {
                self.emit_func_data(format!("    mov rax, [rbp + {}]", stack_pos + i * 8));
                self.emit_func_data(format!("    mov [rbp - {}], rax", local_pos - i * 8));
            }

            if remainder == 0 {
                return;
            }

            let src = stack_pos + full * 8;
            let dst = local_pos - full * 8;
            match remainder {
                4 => {
                    self.emit_func_data(format!("    mov eax, [rbp + {}]", src));
                    self.emit_func_data(format!("    mov DWORD [rbp - {}], eax", dst));
                }
                2 => {
                    self.emit_func_data(format!("    mov ax, [rbp + {}]", src));
                    self.emit_func_data(format!("    mov WORD [rbp - {}], ax", dst));
                }
                1 => {
                    self.emit_func_data(format!("    mov al, [rbp + {}]", src));
                    self.emit_func_data(format!("    mov BYTE [rbp - {}], al", dst));
                }
                _ => {
                    for b in 0..remainder {
                        self.emit_func_data(format!("    mov al, [rbp + {}]", src + b));
                        self.emit_func_data(format!("    mov BYTE [rbp - {}], al", dst - b));
                    }
                }
            }
        } else {
            self.emit_func_data(format!(
                "    mov [rbp - {}], {}",
                local_pos,
                self.reg_for_size(arg_regs[pos - 1], ty).unwrap()
            ));
        }
    }

    pub fn compile_args(&mut self, args: &Vec<Declaration>, ret_type: &Type) {
        let mut is_rvo = false;
        match &self.ensure_monomorphized(ret_type) {
            Type::Enum(name, _) => is_rvo = true,
            Type::Struct(name) => is_rvo = true,
            _ => {}
        }
        let mut arg_index = self.arg_count(is_rvo, args);
        if is_rvo {
            let arg_ty = Type::Primitive(TokenType::I64); // the ptr is 64 bits too so thats fine
            let reg = self.reg_for_size("rdi", &arg_ty).unwrap();
            self.alloc(8);
            self.emit_func_data(format!("    mov [rbp - 8], {}", reg));
        }
        let mut stack_arg_pos = 16;
        for (index, decl) in args.iter().enumerate().rev() {
            let arg_ty = self.ensure_monomorphized(&decl.ty);
            let pos = self.alloc_type(&arg_ty);
            match arg_ty {
                Type::Enum(ref name, _) => {
                    let size = self.enums.borrow().get(name).unwrap().size;
                    if size <= 8 {
                        self.get_arg(arg_index, &arg_ty, pos, Some(stack_arg_pos), is_rvo);
                        arg_index -= 1;
                    } else if size <= 16 && arg_index < 6 {
                        self.get_arg(arg_index, &arg_ty, pos - 8, None, is_rvo);
                        arg_index -= 1;
                        self.get_arg(arg_index, &arg_ty, pos, None, is_rvo);
                        arg_index -= 1
                    } else {
                        self.get_arg(arg_index, &arg_ty, pos, Some(stack_arg_pos), is_rvo);
                        stack_arg_pos += self.type_size(&arg_ty);
                    }
                }
                Type::Struct(ref name) => {
                    let size = self.structs.borrow().get(name).unwrap().size;
                    if size <= 8 {
                        self.get_arg(arg_index, &arg_ty, pos, Some(stack_arg_pos), is_rvo);
                        arg_index -= 1;
                    } else if size <= 16 && arg_index < 6 {
                        self.get_arg(arg_index, &arg_ty, pos - 8, None, is_rvo);
                        arg_index -= 1;
                        self.get_arg(arg_index, &arg_ty, pos, None, is_rvo);
                        arg_index -= 1
                    } else {
                        self.get_arg(arg_index, &arg_ty, pos, Some(stack_arg_pos), is_rvo);
                        stack_arg_pos += self.type_size(&arg_ty);
                    }
                }
                _ => {
                    self.get_arg(arg_index, &arg_ty, pos, Some(stack_arg_pos), is_rvo);
                    if arg_index > 6 {
                        stack_arg_pos += self.type_size(&arg_ty);
                    } else {
                        arg_index -= 1;
                    }
                }
            }

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

    pub fn member_addr(&mut self, base: &Expr, field_name: &str) -> Type {
        let base_type = base.get_type(self);
        self.eval_expr(base, &base_type); // rax = pointer to struct

        let struct_name = match &base_type {
            Type::Pointer(inner) => match inner.as_ref() {
                Type::Struct(name) => name.clone(),
                _ => self::panic!("pointer to non-struct"),
            },
            Type::Struct(name) => name.clone(),
            _ => self::panic!("-> on non-pointer, use . instead"),
        };

        let struct_data = self.structs.borrow().get(&struct_name).unwrap().clone();
        let field = struct_data.elements.get(field_name).unwrap();
        self.emit_func_data(format!("    add rax, {}", field.offset));
        field.ty.clone()
    }

    pub fn gen_func(
        &mut self,
        data: (
            &String,
            &Vec<Declaration>,
            &Type,
            &Box<Stmt>,
            &HashMap<String, Type>,
        ),
    ) {
        let saved_func_out = std::mem::take(&mut self.func_out);
        let saved_func_data = std::mem::take(&mut self.func_data);
        let saved_func_header = std::mem::take(&mut self.func_header);
        let saved_highest_stack_pos = self.highest_stack_pos;
        self.highest_stack_pos = 0;
        let (name, args, ret_type, body, generics) = data;
        self.current_return_type = ret_type.clone();
        // save outer scopes, start fresh with globals only
        let global_scope = self.scopes[0].clone();
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![global_scope]);
        let saved_stack = self.stack_pos;
        let overload_pos = self
            .functions
            .get(name)
            .unwrap()
            .iter()
            .position(|func| {
                func.args.len() == args.len()
                    && args
                        .iter()
                        .enumerate()
                        .all(|(i, decl)| func.args[i].ty == decl.ty)
            })
            .expect(&format!("no matching overload for '{}'", name));
        if self.functions.get(name).unwrap().len() > 1 {
            self.emit_func_header(format!("{}___{}:", name, overload_pos));
        } else {
            self.emit_func_header(format!("{}:", name));
        }
        self.compile_args(args, ret_type);
        self.gen_stmt(body);

        // restore outer scopes
        match ret_type {
            Type::Primitive(ty) if *ty == TokenType::Void => {
                self.emit_func_data("    mov rsp, rbp".to_string());
                self.emit_func_data("    pop rbp".to_string());
                self.emit_func_data("    ret".to_string());
            }
            _ => {}
        }
        self.emit_func_header("    push rbp".to_string());
        self.emit_func_header("    mov rbp, rsp".to_string());
        self.emit_func_header(format!("    sub rsp, {}", align16(self.highest_stack_pos)));

        self.emit_func(self.func_header.clone());
        self.emit_func(self.func_data.clone());
        self.emit_main(self.func_out.clone());
        self.highest_stack_pos = saved_highest_stack_pos;
        self.scopes = saved_scopes;
        self.stack_pos = saved_stack;
        self.func_data = saved_func_data;
        self.func_header = saved_func_header;
        self.func_out = saved_func_out;
    }

    pub fn gen_init_struct(&mut self, data: &StructDef) {
        let mut elements = IndexMap::new();
        for field in &data.fields {
            elements.insert(field.name.clone(), field.clone());
        }
        // the generic structs size would be recomputed anyway
        let size = if data.generic_type.len() > 0 {
            0
        } else {
            self.compute_struct_size(&data.fields)
        };
        let struct_data = StructData {
            name: data.name.clone(),
            generic_type: data.generic_type.clone(),
            elements,
            size,
        };
        let mut structs = self.structs.borrow_mut();
        structs.insert(data.name.clone(), struct_data);
    }

    pub fn compute_struct_size(&self, fields: &Vec<StructField>) -> usize {
        let mut offset = 0;
        let mut max_align = 1;

        for field in fields {
            let ty = self.ensure_monomorphized(&field.ty);
            let align = self.field_alignment(&ty);
            let size = self.type_size(&ty);

            offset = (offset + align - 1) & !(align - 1);
            offset += size;

            if align > max_align {
                max_align = align;
            }
        }

        (offset + max_align - 1) & !(max_align - 1)
    }

    pub fn type_size(&self, ty: &Type) -> usize {
        match ty {
            Type::Primitive(token) => match token {
                TokenType::I8 | TokenType::U8 => 1,
                TokenType::I16 | TokenType::U16 => 2,
                TokenType::I32 | TokenType::U32 => 4,
                TokenType::I64 | TokenType::U64 => 8,
                _ => self::panic!("Unsupported primitive type: {:?}", token),
            },
            Type::Pointer(_) => 8,
            Type::Array(elem_type, count) => self.type_size(elem_type) * count,
            Type::Struct(name) => self.ensure_struct_sized(name),
            Type::Enum(name, _) => self.ensure_enum_sized(name),
            Type::GenericType(name) => {
                let ty = {
                    let map = self.generics.borrow();
                    map.get(name).cloned().unwrap()
                };
                self.type_size(&ty)
            }
            Type::Named(name) => {
                if self.structs.borrow().contains_key(name) {
                    self.ensure_struct_sized(name)
                } else if self.enums.borrow().contains_key(name) {
                    self.ensure_enum_sized(name)
                } else {
                    self::panic!("unknown type: {}", name);
                }
            }
            Type::Unknown | Type::GenericInst(..) => {
                println!("{:?}", ty);
                self::panic!("unkown type")
            }
        }
    }

    fn type_to_data_directive(&self, ty: &Type) -> &str {
        match self.type_size(ty) {
            8 => "dq",
            4 => "dd",
            2 => "dw",
            1 => "db",
            _ => {
                println!("warning unkown type: {:?} ", ty);
                return "dq";
            } // default to 8 for unknown/structs/arrays
        }
    }

    fn size_directive(&self, ty: &Type) -> &str {
        match self.type_size(ty) {
            8 => "resq",
            4 => "resd",
            2 => "resw",
            1 => "resb",
            _ => {
                println!("warning unkown type: {:?} ", ty);
                return "resq";
            } // default to 8 for unknown/structs/arrays
        }
    }

    fn gen_global(&mut self, global: Box<Stmt>) {
        match &global.ty {
            StmtType::Declaration(decl_data) => {
                if let Some(_) = decl_data.initializer {
                    self::panic!("global cant have expr");
                }
                match &decl_data.ty {
                    Type::Array(ty, size) => {
                        self.emit_bss(format!("{} {} 0", decl_data.name, self.size_directive(&ty)));
                    }
                    _ => {
                        self.emit_data(format!(
                            "{} {} 0",
                            decl_data.name,
                            self.type_to_data_directive(&decl_data.ty)
                        ));
                    }
                }
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
                            self.emit_func_data(format!("    mov [rel {}], rax", decl_data.name));
                        }
                        _ => {}
                    }
                }
            }
            _ => self::panic!("trying to make global of strange stmt"),
        }
    }

    fn gen_match_field_arg(
        &mut self,
        var_ty: &Type,
        field: &StructField,
        reg: &String,
        pos: usize,
    ) {
        match var_ty {
            Type::Primitive(_) => match field.ty {
                Type::Primitive(_) | Type::Array(..) => {
                    self.emit_func_data(format!("    mov {reg}, [rbp - {pos}]"));
                }
                Type::Unknown => {
                    self::panic!("some error");
                }
                _ => {
                    self.emit_func_data(format!("    lea {reg}, [rbp - {pos}]"));
                }
            },
            Type::Enum(..) => {
                self.emit_func_data(format!("    mov rax, [rbp - {}]", pos));
                self.emit_func_data(format!("    add rax, {}", field.offset));
                self.emit_func_data(format!("    mov rax, [rax]"));
            }
            Type::Pointer(_) => {
                self.emit_func_data(format!("    mov rax, [rbp - {}]", pos));
                self.emit_func_data(format!("    add rax, {}", field.offset));
                self.emit_func_data(format!("    mov rax, [rax]"));
            }
            _ => {
                self::panic!("match arg error: {:?}", var_ty);
            }
        }
    }

    fn gen_match_field(&mut self, variant: &MatchField, base_pos: usize, expr_ty: &Type) {
        match &variant.left {
            MatchLeftValue::Enum { base, value, args } => {
                if base == "_" || args.len() < 1 {
                    self.gen_stmt(&variant.right);
                    return;
                }
                let new_base: &String = {
                    match expr_ty {
                        Type::Enum(name, _) => name,
                        _ => base,
                    }
                };
                let enum_data = self.enums.borrow().get(new_base).unwrap().clone();
                let field_data = enum_data.variants.get(value).unwrap();
                for (index, arg) in args.iter().enumerate() {
                    let field = &field_data.args[index];
                    let ty = {
                        match field.ty {
                            Type::Struct(_) | Type::Enum(..) => {
                                Type::Pointer(Box::new(field.ty.clone()))
                            }
                            _ => field.ty.clone(),
                        }
                    };
                    let decl = Declaration {
                        name: arg.clone(),
                        ty: ty,
                        initializer: None,
                    };
                    self.gen_declaration(&decl);
                    let new_var_pos = self.stack_pos;
                    // the tag size
                    let pos = base_pos;
                    let reg = self.reg_for_size("rax", &field.ty).unwrap();
                    self.gen_match_field_arg(expr_ty, &field, &reg, pos);
                    self.emit_func_data(format!("    mov [rbp - {new_var_pos}], {reg}"));
                }
                self.gen_stmt(&variant.right);
            }
            MatchLeftValue::Expr { .. } => {
                self.gen_stmt(&variant.right);
            }
        }
    }

    fn gen_match_asm_checking(&mut self, var: &MatchField, id: usize, expr_ty: &Type) {
        match &var.left {
            MatchLeftValue::Expr { expr } => match expr.ty {
                ExprType::Number(num) => {
                    self.emit_func_data(format!("    cmp rax, {num}"));
                    self.emit_func_data(format!("    je match_{}_{}", id, num));
                }
                _ => self::panic!("match field left value not supported"),
            },
            MatchLeftValue::Enum { base, value, args } => {
                if base == "_" {
                    self.emit_func_data(format!("    jmp match_{id}_wildcard"));
                    return;
                }
                let new_base = {
                    match expr_ty {
                        Type::Enum(name, _) => name,
                        _ => base,
                    }
                };
                let enum_data = self.enums.borrow().get(new_base).cloned().unwrap();
                let field_data = enum_data.variants.get(value).unwrap();
                let tag = field_data.tag;
                self.emit_func_data(format!("    cmp rax, {}", tag));
                self.emit_func_data(format!("    je match_{}_{}", id, tag));
            }
        }
    }

    fn gen_match_asm_func(
        &mut self,
        variant: &MatchField,
        id: usize,
        base_pos: usize,
        expr_ty: &Type,
    ) {
        match &variant.left {
            MatchLeftValue::Expr { expr } => match expr.ty {
                ExprType::Number(num) => {
                    self.emit_func_data(format!("match_{}_{}:", id, num));
                }
                _ => self::panic!("not supported"),
            },
            MatchLeftValue::Enum { base, value, args } => {
                if base == "_" {
                    self.emit_func_data(format!("match_{id}_wildcard:"));
                } else {
                    let new_base = {
                        match expr_ty {
                            Type::Enum(name, _) => name,
                            _ => base,
                        }
                    };
                    let enum_data = self.enums.borrow().get(new_base).cloned().unwrap();
                    let field_data = enum_data.variants.get(value).unwrap();
                    let tag = field_data.tag;
                    self.emit_func_data(format!("match_{id}_{tag}:"));
                }
            }
        }
        self.scopes.push(HashMap::new());
        self.gen_match_field(&variant, base_pos, expr_ty);
        self.emit_func_data(format!("    jmp match_end_{id}"));
        self.scopes.pop();
    }

    fn resolve_match_expr(
        &mut self,
        base_pos: usize,
        variants: &Vec<MatchField>,
        id: usize,
        expr_ty: &Type,
    ) {
        //match expr_ty
        for var in variants {
            self.gen_match_asm_checking(var, id, &expr_ty);
        }
        self.emit_func_data(format!("    jmp match_end_{id}"));
        for var in variants {
            self.gen_match_asm_func(var, id, base_pos, &expr_ty);
        }
        self.emit_func_data(format!("match_end_{}:", id));
    }

    fn gen_match(&mut self, expr: &Expr, variants: &Vec<MatchField>) {
        let id = self.get_id();
        let expr_ty = &expr.get_type(self);
        let reg = self.eval_expr(expr, expr_ty);
        let pos = self.alloc(8);
        self.emit_func_data(format!("    mov [rbp - {}], rax", pos));
        let expr_ty = expr.get_type(self);
        match expr_ty {
            Type::Enum(..) => {
                self.emit_func_data(format!("    mov rax, [rax]"));
            }
            Type::Pointer(..) => self.emit_func_data(format!("    mov rax, [rax]")),
            _ => {}
        }
        self.resolve_match_expr(pos, variants, id, &expr_ty);
        self.stack_pos -= 8;
    }

    fn gen_extern(&mut self, function: &Box<Stmt>) {
        match &function.ty {
            StmtType::InitFunc {
                name,
                generic_types,
                args,
                ret_type,
                data,
            } => {
                let extern_func_data = FuncData {
                    args: args.to_vec(),
                    generic: Vec::new(),
                    return_type: ret_type.clone(),
                };
                self.emit(format!("extern {}", name));
                self.functions.insert(name.clone(), vec![extern_func_data]);
            }
            _ => self::panic!("the extern is not a function: {:?}", function),
        }
    }

    fn gen_break(&mut self) {
        let path = self.break_stack.last().unwrap();
        self.emit_func_data(format!("    jmp {}", path));
    }

    fn gen_contniue(&mut self) {
        let path = self.contniue_stack.last().unwrap();
        self.emit_func_data(format!("    jmp {}", path));
    }

    pub fn gen_stmt(&mut self, stmt: &Stmt) {
        match &stmt.ty {
            StmtType::Break => self.gen_break(),
            StmtType::Continue => self.gen_contniue(),
            StmtType::Block(v) => self.gen_block(v),
            StmtType::Declaration(v) => self.gen_declaration(v),
            StmtType::Assignment { target, value } => self.gen_assignment(target, value),
            StmtType::ExprStmt(expr) => {
                self.eval_expr(expr, &Type::Primitive(TokenType::I64));
            }
            StmtType::If {
                condition,
                if_block,
                else_block,
            } => {
                self.gen_if((condition, if_block, else_block));
            }
            StmtType::While { condition, body } => {
                self.gen_while((condition, body));
            }
            StmtType::For {
                init,
                condition,
                update,
                body,
            } => {
                self.gen_for((init, condition, update, body));
            }
            StmtType::Return(expr) => self.gen_ret(expr),
            StmtType::AsmCode(data) => self.gen_inline_asm(data),
            StmtType::GenericInitFunc {
                name,
                generic_types,
                args,
                ret_type,
                data,
            } => {}
            StmtType::InitFunc {
                name,
                args,
                ret_type,
                data,
                generic_types,
            } => self.gen_func((name, args, ret_type, data, generic_types)),
            StmtType::InitStruct(..) => {} // skiping because we already added it in first iteration,
            StmtType::GlobalDecl(global) => self.gen_global(global.clone()),
            StmtType::InitEnum { .. } => {}
            StmtType::Match { expr, variants } => self.gen_match(expr, variants),
            StmtType::ExternFn(function) => self.gen_extern(function),
        }
    }
}
