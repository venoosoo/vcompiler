use crate::Ir::expr::{BinOp, Expr, UnaryOp};
use crate::sem_analysis::{coerce_numeric, is_numeric};

use super::*;
use super::{get_word, reg_for_size};

// TODO needs rewrite to support both Gen and Analyzer
// needs traits
impl Expr {
    /// Returns the Type of this expression
    pub fn get_type_of_expr(&self, gen_helper: &Gen) -> Type {
        match self {
            Expr::Number(_) => Type::Primitive(TokenType::LongType),
            Expr::Float(_) => self::panic!("floats are not implemented"),
            Expr::Variable(name) => {
                if gen_helper.structs.get(name).is_some() {
                    return Type::Struct(name.clone());
                } else {
                    let var = gen_helper.lookup_var(name);
                    var.var_type.clone()
                }
            }
            Expr::Unary { op, expr } => {
                match op {
                    UnaryOp::Neg => expr.get_type_of_expr(gen_helper),
                    UnaryOp::Not => Type::Primitive(TokenType::CharType), // boolean
                }
            }
            Expr::Binary { op: _, left, right } => {
                let lty = left.get_type_of_expr(gen_helper);
                let rty = right.get_type_of_expr(gen_helper);
                coerce_numeric(&lty, &rty)
            }
            Expr::StructInit {
                struct_name_ty,
                fields: _,
            } => {
                if let Some(_struct_data) = gen_helper.structs.get(struct_name_ty) {
                    Type::Struct(struct_name_ty.clone())
                } else {
                    self::panic!("Struct {} not found in get_type_of_expr", struct_name_ty);
                }
            }

            Expr::Deref(ptr_expr) => match ptr_expr.get_type_of_expr(gen_helper) {
                Type::Pointer(inner) => *inner,
                _ => self::panic!("Cannot dereference a non-pointer"),
            },
            Expr::AddressOf(var_expr) => {
                let ty = var_expr.get_type_of_expr(gen_helper);
                Type::Pointer(Box::new(ty))
            }
            Expr::Index { base, index } => {
                let base_ty = base.get_type_of_expr(gen_helper);
                let idx_ty = index.get_type_of_expr(gen_helper);
                if !is_numeric(&idx_ty) {
                    self::panic!("Array index must be integer");
                }
                match base_ty {
                    Type::Array(elem_ty, _) => *elem_ty,
                    Type::Pointer(elem_ty) => *elem_ty,
                    _ => self::panic!("Cannot index into non-array type"),
                }
            }
            Expr::StructMember { base, name } => {
                let base_ty = base.get_type_of_expr(gen_helper);
                let struct_name = match &base_ty {
                    Type::Struct(n) => n.clone(),
                    Type::Pointer(inner) => match inner.as_ref() {
                        Type::Struct(n) => n.clone(),
                        _ => self::panic!("pointer to non-struct"),
                    },
                    _ => self::panic!("member access on non-struct"),
                };
                let struct_data = gen_helper.structs.get(&struct_name).unwrap();
                let field = struct_data.elements.get(name).unwrap();
                field.ty.clone()
            }
            Expr::Call { name, args: _ } => {
                let func_data = gen_helper
                    .functions
                    .get(name)
                    .expect(&format!("Function {} not found", name));
                func_data.return_type.clone()
            }
            Expr::ArrayInit { elements } => {
                if elements.len() > 0 {
                    return elements[0].get_type_of_expr(gen_helper);
                } else {
                    Type::Unknown
                }
            }
            Expr::SizeOf { ty } => Type::Primitive(TokenType::LongType),
            Expr::String { str } => {
                return Type::Array(
                    Box::new(Type::Primitive(TokenType::CharType)),
                    str.len() + 1,
                );
            }
        }
    }
}

impl Gen {
    fn gen_expr_binop(
        &mut self,
        op: &BinOp,
        left_reg: &str,  // rbx/ebx etc
        right_reg: &str, // rax/eax etc
        expected_type: &Type,
    ) {
        let result_reg = reg_for_size("rax", expected_type).unwrap();
        let left_sized = reg_for_size("rbx", expected_type).unwrap();

        match op {
            BinOp::Add => {
                self.emit_main(format!("    add {}, {}", left_reg, right_reg));
                self.emit_main(format!("    mov {}, {}", result_reg, left_sized));
            }
            BinOp::Sub => {
                self.emit_main(format!("    sub {}, {}", left_reg, right_reg));
                self.emit_main(format!("    mov {}, {}", result_reg, left_sized));
            }
            BinOp::Mul => {
                self.emit_main(format!("    imul {}, {}", left_reg, right_reg));
                self.emit_main(format!("    mov {}, {}", result_reg, left_sized));
            }
            BinOp::Div => {
                if self.type_size(expected_type) == 8 {
                    self.emit_main("    cqo".to_string());
                } else {
                    self.emit_main("    cdq".to_string());
                }
                self.emit_main(format!(
                    "    idiv {}",
                    reg_for_size("rbx", expected_type).unwrap()
                ));
                // result already in rax
            }
            BinOp::Mod => {
                if self.type_size(expected_type) == 8 {
                    self.emit_main("    cqo".to_string());
                } else {
                    self.emit_main("    cdq".to_string());
                }
                self.emit_main(format!(
                    "    idiv {}",
                    reg_for_size("rbx", expected_type).unwrap()
                ));
                // remainder in rdx, move to rax
                self.emit_main(format!(
                    "    mov {}, {}",
                    result_reg,
                    reg_for_size("rdx", expected_type).unwrap()
                ));
            }
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                self.emit_main(format!("    cmp {}, {}", right_reg, left_reg));
                let set_instr = match op {
                    BinOp::Eq => "sete",
                    BinOp::Neq => "setne",
                    BinOp::Lt => "setl",
                    BinOp::Lte => "setle",
                    BinOp::Gt => "setg",
                    BinOp::Gte => "setge",
                    _ => unreachable!(),
                };
                self.emit_main(format!("    {} al", set_instr));
                self.emit_main(format!("    movzx {}, al", result_reg));
            }
            BinOp::And => {
                self.emit_main(format!("    cmp {}, 0", left_reg));
                self.emit_main("    setne al".to_string());
                self.emit_main(format!("    cmp {}, 0", right_reg));
                self.emit_main("    setne dl".to_string());
                self.emit_main("    and al, dl".to_string());
                self.emit_main(format!("    movzx {}, al", result_reg));
            }
            BinOp::Or => {
                self.emit_main(format!("    cmp {}, 0", left_reg));
                self.emit_main("    setne al".to_string());
                self.emit_main(format!("    cmp {}, 0", right_reg));
                self.emit_main("    setne dl".to_string());
                self.emit_main("    or al, dl".to_string());
                self.emit_main(format!("    movzx {}, al", result_reg));
            }
        }
    }

    fn push_result(&mut self) {
        self.emit_main("    push rax".to_string());
    }
    fn pop_into(&mut self, reg: &str) {
        self.emit_main(format!("    pop {}", reg));
    }

    fn gen_expr_num(&mut self, num: &i64, expected_type: &Type) -> String {
        let sized_rax = reg_for_size("rax", expected_type).unwrap();
        self.emit_main(format!("    mov {}, {}", sized_rax, num));
        "rax".to_string()
    }

    fn gen_expr_var(&mut self, var_name: &String, expected_type: &Type) -> String {
        let var_data = self.lookup_var(var_name);

        if var_data.global_flag {
            match var_data.var_type {
                Type::Primitive(_) | Type::Pointer(_) => {
                    let sized_rax = reg_for_size("rax", &var_data.var_type).unwrap();
                    self.emit_main(format!("    mov {}, [rel {}]", sized_rax, var_name));
                }
                _ => {
                    // struct/array — load address
                    self.emit_main(format!("    lea rax, [rel {}]", var_name));
                }
            }
            return "rax".to_string();
        }

        match var_data.var_type {
            Type::Primitive(_) => {
                let actual_size = self.type_size(&var_data.var_type);
                let expected_size = self.type_size(expected_type);
                if expected_size > actual_size {
                    let src_word = get_word(&var_data.var_type);
                    self.emit_main(format!(
                        "    movsx rax, {} [rbp - {}]",
                        src_word, var_data.stack_pos
                    ));
                } else {
                    let sized_rax = reg_for_size("rax", &var_data.var_type).unwrap();
                    self.emit_main(format!(
                        "    mov {}, {} [rbp - {}]",
                        sized_rax,
                        get_word(&var_data.var_type),
                        var_data.stack_pos
                    ));
                }
            }
            Type::Pointer(_) => {
                self.emit_main(format!("    mov rax, [rbp - {}]", var_data.stack_pos));
            }
            _ => {
                // struct/array — load address
                self.emit_main(format!("    lea rax, [rbp - {}]", var_data.stack_pos));
            }
        }
        "rax".to_string()
    }

    fn gen_expr_binary(
        &mut self,
        data: (&BinOp, &Box<Expr>, &Box<Expr>),
        expected_type: &Type,
    ) -> String {
        let (op, left, right) = data;
        self.eval_expr(right, expected_type);
        self.push_result();
        self.eval_expr(left, expected_type);
        self.pop_into("rbx");

        let left_reg = reg_for_size("rbx", &expected_type).unwrap(); // e.g. ebx
        let right_reg = reg_for_size("rax", &expected_type).unwrap();

        self.gen_expr_binop(op, &left_reg, &right_reg, expected_type);

        left_reg
    }

    fn gen_expr_unary(&mut self, op: &UnaryOp, expr: &Box<Expr>, expected_type: &Type) -> String {
        match op {
            UnaryOp::Neg => {
                self.eval_expr(expr, expected_type);
                let sized = reg_for_size("rax", expected_type).unwrap();
                self.emit_main(format!("    neg {}", sized));
            }
            UnaryOp::Not => {
                self.eval_expr(expr, expected_type);
                let sized = reg_for_size("rax", expected_type).unwrap();
                self.emit_main(format!("    cmp {}, 0", sized));
                self.emit_main("    sete al".to_string());
                self.emit_main(format!("    movzx {}, al", sized));
            }
        }
        "rax".to_string()
    }

    fn gen_expr_call(&mut self, name: &String, args: &Vec<Expr>, expected_type: &Type) -> String {
        let func_data = self.functions.get(name).unwrap().clone();
        for (index, arg) in args.iter().enumerate() {
            let arg_type = func_data.args[index].ty.clone();
            self.eval_expr(arg, &arg_type);
            self.emit_main("    push rax".to_string());
        }

        // pop into arg registers in reverse order
        for (index, arg) in args.iter().enumerate().rev() {
            let arg_type = func_data.args[index].ty.clone();
            let arg_reg = arg_pos(index, &arg_type);
            self.emit_main(format!("    pop {}", to_base_reg(&arg_reg)));
            // then size it down if needed
            let sized = reg_for_size(&to_base_reg(&arg_reg), &arg_type).unwrap();
            // pop already put it in the right base reg, sized is just for reference
        }

        self.emit_main(format!("    call {}", name));
        return "rax".to_string();
    }

    fn gen_expr_struct_init(
        &mut self,
        fields: &Vec<(String, Expr)>,
        struct_name: &String,
    ) -> String {
        let struct_data = self
            .structs
            .get(struct_name)
            .expect("Unknown struct")
            .clone();
        let base_pos = self.stack_pos;

        for (field_name, field_expr) in fields {
            let field = struct_data.elements.get(field_name).expect("Unknown field");
            let field_type = &field.ty;
            self.eval_expr(field_expr, field_type); // ← field_type not expected_type
            let sized_reg = reg_for_size("rax", field_type).unwrap();
            let size_word = get_word(field_type);
            let field_pos = base_pos - field.offset;
            self.emit_main(format!(
                "    mov {} [rbp - {}], {}",
                size_word, field_pos, sized_reg
            ));
        }
        "rax".to_string()
    }

    fn gen_expr_struct_member(&mut self, base: &Box<Expr>, name: &String) -> String {
        let base_type = base.get_type_of_expr(self);

        let struct_name = match &base_type {
            Type::Struct(n) => n.clone(),
            _ => self::panic!("member access on non-struct"),
        };

        let field = self
            .structs
            .get(&struct_name)
            .unwrap()
            .elements
            .get(name)
            .unwrap()
            .clone();
        let size_word = get_word(&field.ty);

        match base.as_ref() {
            Expr::Deref(inner) => {
                // -> operator: eval inner to get pointer value, add offset, read
                self.eval_expr(inner, &Type::Pointer(Box::new(base_type.clone())));
                self.emit_main(format!("    add rax, {}", field.offset));
                self.emit_main(format!("    mov rax, {} [rax]", size_word));
            }
            Expr::Variable(var_name) => {
                // . operator: compile-time offset
                let var = self.lookup_var(var_name);
                let field_addr = var.stack_pos - field.offset;
                self.emit_main(format!("    mov rax, {} [rbp - {}]", size_word, field_addr));
            }
            _ => {
                // chained a.b.c — runtime fallback
                self.eval_expr(base, &base_type);
                self.emit_main(format!("    add rax, {}", field.offset));
                self.emit_main(format!("    mov rax, {} [rax]", size_word));
            }
        }
        "rax".to_string()
    }

    fn gen_expr_deref(&mut self, expr: &Box<Expr>, expected_type: &Type) -> String {
        self.eval_expr(expr, expected_type);
        let size_word = get_word(expected_type);
        let sized_rax = reg_for_size("rax", expected_type).unwrap();

        match expected_type {
            Type::Primitive(TokenType::IntType)
            | Type::Primitive(TokenType::ShortType)
            | Type::Primitive(TokenType::CharType) => {
                self.emit_main(format!("    movsx rax, {} [rax]", size_word));
            }
            _ => {
                self.emit_main(format!("    mov {}, {} [rax]", sized_rax, size_word));
            }
        }
        "rax".to_string()
    }

    fn gen_expr_addres_of(&mut self, expr: &Box<Expr>) -> String {
        match &**expr {
            Expr::Variable(name) => {
                let var = self.lookup_var(name);
                if var.global_flag {
                    self.emit_main(format!("    lea rax, [rel {}]", name));
                } else {
                    self.emit_main(format!("    lea rax, [rbp - {}]", var.stack_pos));
                }
                "rax".to_string()
            }

            Expr::StructMember { base, name } => {
                self.member_addr(base, name);
                "rax".to_string()
            }

            Expr::Index { base, index } => {
                let elem_type = expr.get_type_of_expr(self);
                let elem_size = self.type_size(&elem_type);
                let base_type = base.get_type_of_expr(self);

                // eval base first, push it
                self.eval_expr(base, &base_type);
                self.push_result();

                // eval index, scale it
                self.eval_expr(index, &Type::Primitive(TokenType::LongType));
                self.emit_main(format!("    imul rax, rax, {}", elem_size));

                // pop base, add scaled index
                self.pop_into("rbx");
                self.emit_main("    add rax, rbx".to_string());
                "rax".to_string()
            }

            Expr::Deref(inner) => {
                // &*ptr == ptr
                let ptr_type = Type::Pointer(Box::new(expr.get_type_of_expr(self)));
                self.eval_expr(inner, &ptr_type)
            }

            _ => self::panic!("Cannot take address of this expression"),
        }
    }

    fn gen_expr_index(
        &mut self,
        base: &Box<Expr>,
        index: &Box<Expr>,
        expected_type: &Type,
    ) -> String {
        let arr_ty = &base.get_type_of_expr(self);
        self.eval_expr(base, arr_ty);
        self.push_result();
        self.eval_expr(index, &Type::Primitive(TokenType::LongType));

        //runtime checking
        match arr_ty {
            Type::Array(ty, size) => {
                self.emit_main(format!("    cmp rax, {}", size));
                self.emit_main(format!("    jge __bounds_fail__"));
                self.emit_main(format!("    cmp rax, 0"));
                self.emit_main(format!("    jl __bounds_fail__"));
            }
            _ => {}
        }

        let elem_size = self.type_size(expected_type);
        self.emit_main(format!("    imul rax, rax, {}", elem_size,));
        self.pop_into("rbx");
        self.emit_main(format!("    add rax, rbx"));
        let size_word = get_word(&expected_type);
        match &expected_type {
            Type::Primitive(TokenType::CharType) | 
            Type::Primitive(TokenType::ShortType) |
            Type::Primitive(TokenType::IntType) => {
                self.emit_main(format!("    movsx rax, {} [rax]", size_word));
            }
            _ => {
                self.emit_main(format!("    mov rax, {} [rax]", size_word));
            }
        }
        "rax".to_string()
    }

    fn gen_array_init(&mut self, elements: &Vec<Expr>, expected_type: &Type) -> String {
        let elem_type = match expected_type {
            Type::Array(elem_ty, _) => *elem_ty.clone(),
            _ => self::panic!("gen_array_init called with non-array type"),
        };
        let elem_size = self.type_size(&elem_type);
        let base_pos = self.stack_pos;

        for (i, elem) in elements.iter().enumerate() {
            self.eval_expr(elem, &elem_type);
            let sized_reg = reg_for_size("rax", &elem_type).unwrap();
            let size_word = get_word(&elem_type);
            let offset = base_pos - (i * elem_size);

            self.emit_main(format!(
                "    mov {} [rbp - {}], {}",
                size_word, offset, sized_reg
            ));
        }
        self.emit_main(format!("    lea rax, [rbp - {}]", base_pos));
        "rax".to_string()
    }

    fn gen_size_of(&mut self, stmt: &Box<Stmt>) -> String {
        let ty = {
            match *stmt.clone() {
                Stmt::Declaration(decl) => decl.ty,
                _ => self::panic!("bug"),
            }
        };
        let size = self.type_size(&ty);
        self.emit_main(format!("    mov rax, {}", size));
        "rax".to_string()
    }

    fn gen_string(&mut self, str: &String) -> String {
        let expected_ty = Type::Array(
            Box::new(Type::Primitive(TokenType::CharType)),
            str.len() + 1,
        );
        let mut expr_vec = Vec::new();
        for char in str.chars() {
            let expr = Expr::Number(char as i64);
            expr_vec.push(expr);
        }
        expr_vec.push(Expr::Number(0));
        let reg = self.gen_array_init(&expr_vec, &expected_ty);
        reg
    }

    pub fn eval_expr(&mut self, expr: &Expr, expected_type: &Type) -> String {
        match expr {
            Expr::ArrayInit { elements } => self.gen_array_init(elements, expected_type),
            Expr::Number(num) => self.gen_expr_num(num, expected_type),

            Expr::Variable(var) => self.gen_expr_var(var, expected_type),

            Expr::Binary { op, left, right } => {
                let expr_ty = expr.get_type_of_expr(self);
                self.gen_expr_binary((op, left, right), &expr_ty)
            }

            Expr::Unary { op, expr: inner } => self.gen_expr_unary(op, inner, expected_type),

            Expr::Call { name, args } => {
                let ret_ty = self.functions.get(name).unwrap().return_type.clone();
                self.gen_expr_call(name, args, &ret_ty)
            }

            Expr::Deref(inner) => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_deref(inner, &ty)
            }

            Expr::AddressOf(inner) => self.gen_expr_addres_of(inner),

            Expr::Index { base, index } => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_index(base, index, &ty)
            }

            Expr::StructMember { base, name } => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_struct_member(base, name)
            }

            Expr::StructInit {
                fields,
                struct_name_ty,
            } => self.gen_expr_struct_init(fields, struct_name_ty),

            Expr::SizeOf { ty } => self.gen_size_of(ty),
            Expr::Float(_) => self::panic!("floats not implemented"),
            Expr::String { str } => self.gen_string(str),
        }
    }
}
