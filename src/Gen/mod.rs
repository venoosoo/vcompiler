use core::panic;
use std::{collections::HashMap, fmt::Write};

use crate::Ir::Stmt;
use crate::Ir::expr::Expr;
use crate::Ir::r#gen::*;
use crate::Ir::sem_analysis::Analyzer;
use crate::Ir::stmt::LValue;
use crate::Ir::stmt::Type;
use crate::Tokenizer::TokenType;

use crate::Ir::sem_analysis::SemanticError;

mod gen_expr;
mod gen_stmt;

pub struct Gen {
    stmts: Vec<Stmt>,
    m_out: String,
    stack_pos: usize,
    current_return_type: Type,
    scopes: Vec<HashMap<String, VarData>>,
    structs: HashMap<String, StructData>,
    functions: HashMap<String, FuncData>,
    id: usize,
}

fn align16(n: usize) -> usize {
    (n + 15) & !15
}

pub fn reg_for_size(base: &str, ty: &Type) -> String {
    let size = match ty {
        Type::Primitive(token) => match token {
            TokenType::CharType => 1,
            TokenType::ShortType => 2,
            TokenType::IntType => 4,
            TokenType::LongType => 8,
            _ => panic!("Unsupported type"),
        },
        Type::Unknown => panic!("unkown type"),
        Type::Pointer(_) | Type::Array(_, _) | Type::Struct(_) => 8,
    };

    match (base, size) {
        ("rax", 8) => "rax".into(),
        ("rax", 4) => "eax".into(),
        ("rax", 2) => "ax".into(),
        ("rax", 1) => "al".into(),

        ("rbx", 8) => "rbx".into(),
        ("rbx", 4) => "ebx".into(),
        ("rbx", 2) => "bx".into(),
        ("rbx", 1) => "bl".into(),

        ("rcx", 8) => "rcx".into(),
        ("rcx", 4) => "ecx".into(),
        ("rcx", 2) => "cx".into(),
        ("rcx", 1) => "cl".into(),

        ("rdx", 8) => "rdx".into(),
        ("rdx", 4) => "edx".into(),
        ("rdx", 2) => "dx".into(),
        ("rdx", 1) => "dl".into(),

        ("rsi", 8) => "rsi".into(),
        ("rsi", 4) => "esi".into(),
        ("rsi", 2) => "si".into(),
        ("rsi", 1) => "sil".into(),

        ("rdi", 8) => "rdi".into(),
        ("rdi", 4) => "edi".into(),
        ("rdi", 2) => "di".into(),
        ("rdi", 1) => "dil".into(),

        // r8–r15 follow predictable pattern
        (reg, 8) => reg.to_string(),
        (reg, 4) if reg.starts_with('r') => format!("{}d", reg),
        (reg, 2) if reg.starts_with('r') => format!("{}w", reg),
        (reg, 1) if reg.starts_with('r') => format!("{}b", reg),

        _ => panic!("Unsupported register: {}", base),
    }
}

pub fn arg_pos(pos: usize, ty: &Type) -> String {
    let size = match ty {
        Type::Primitive(token) => match token {
            TokenType::CharType => 1,
            TokenType::ShortType => 2,
            TokenType::IntType => 4,
            TokenType::LongType => 8,
            _ => panic!("unsupported primitive type in arg_pos: {:?}", token),
        },
        Type::Unknown => panic!("unkown type"),
        Type::Pointer(_) | Type::Array(_, _) | Type::Struct(_) => 8,
    };

    match (pos, size) {
        (0, 8) => "rdi",
        (0, 4) => "edi",
        (0, 2) => "di",
        (0, 1) => "dil",
        (1, 8) => "rsi",
        (1, 4) => "esi",
        (1, 2) => "si",
        (1, 1) => "sil",
        (2, 8) => "rdx",
        (2, 4) => "edx",
        (2, 2) => "dx",
        (2, 1) => "dl",
        (3, 8) => "rcx",
        (3, 4) => "ecx",
        (3, 2) => "cx",
        (3, 1) => "cl",
        (4, 8) => "r8",
        (4, 4) => "r8d",
        (4, 2) => "r8w",
        (4, 1) => "r8b",
        (5, 8) => "r9",
        (5, 4) => "r9d",
        (5, 2) => "r9w",
        (5, 1) => "r9b",
        (6, 8) => "r10",
        (6, 4) => "r10d",
        (6, 2) => "r10w",
        (6, 1) => "r10b",
        (7, 8) => "r11",
        (7, 4) => "r11d",
        (7, 2) => "r11w",
        (7, 1) => "r11b",
        _ => panic!("arg_pos: unsupported pos={} size={}", pos, size),
    }
    .to_string()
}

pub fn get_word(ty: &Type) -> String {
    match ty {
        Type::Primitive(token) => match token {
            TokenType::CharType => "BYTE".to_string(),
            TokenType::ShortType => "WORD".to_string(),
            TokenType::IntType => "DWORD".to_string(),
            TokenType::LongType => "QWORD".to_string(),
            _ => panic!("Unsupported primitive type: {:?}", token),
        },
        Type::Pointer(_) => "QWORD".to_string(), // 64-bit pointer
        Type::Array(_, _) => "QWORD".to_string(), // arrays decay to pointer for memory access
        Type::Struct(struct_name) => "QWORD".to_string(),
        Type::Unknown => panic!("unkown type"),
    }
}

pub fn lvalue_root(lvalue: &LValue) -> String {
    match lvalue {
        LValue::Variable(name) => name.clone(),
        LValue::Field { base, .. } => lvalue_root(base),
        LValue::Deref(inner) => lvalue_root(inner),
        LValue::Index { base, .. } => lvalue_root(base),
    }
}

impl Gen {
    pub fn new(stmts: Vec<Stmt>) -> Gen {
        Gen {
            stmts,
            current_return_type: Type::Primitive(TokenType::IntType),
            m_out: String::new(),
            scopes: vec![HashMap::new()],
            stack_pos: 0,
            structs: HashMap::new(),
            functions: HashMap::new(),
            id: 0,
        }
    }

    fn emit(&mut self, s: String) {
        let _ = writeln!(self.m_out, "{}", s);
    }

    fn get_id(&mut self) -> usize {
        self.id += 1;
        self.id
    }

    fn get_primitive_size(&self, token: &TokenType) -> usize {
        match token {
            TokenType::IntType => 4,
            TokenType::CharType => 1,
            TokenType::ShortType => 2,
            TokenType::LongType => 8,
            _ => panic!("trying to get size of unexpected type: {:?}", token),
        }
    }

    pub fn expr_to_lvalue(&self, expr: &Expr) -> LValue {
        match expr {
            Expr::Variable(name) => LValue::Variable(name.clone()),
            Expr::StructMember { base, name } => LValue::Field {
                base: Box::new(self.expr_to_lvalue(base)),
                name: name.clone(),
            },
            Expr::Index { base, index } => LValue::Index {
                base: Box::new(self.expr_to_lvalue(base)),
                index: Box::new(*index.clone()), // or Box::new(index.clone())
            },
            Expr::Deref(inner) => LValue::Deref(Box::new(self.expr_to_lvalue(inner))),
            _ => panic!("Cannot convert expr to lvalue: {:?}", expr),
        }
    }

    fn get_size(&self, token: &Type) -> usize {
        match token {
            Type::Primitive(ty) => self.get_primitive_size(ty),
            Type::Pointer(_) => 8,
            Type::Array(arr_type, size) => {
                let type_size = self.get_size(&*arr_type);
                size * type_size
            }
            Type::Struct(struct_name) => {
                let struct_data = self
                    .structs
                    .get(struct_name)
                    .expect(&format!("no struct with name {}", struct_name));
                println!("struct: {:?}", struct_data);
                struct_data.element_size
            }
            Type::Unknown => panic!("unkown type"),
        }
    }

    fn alloc_type(&mut self, ty: &Type) -> usize {
        let size: usize = self.get_size(ty);
        self.stack_pos += size;
        self.stack_pos
    }

    fn alloc(&mut self, size: usize) -> usize {
        self.stack_pos += size;
        self.stack_pos
    }

    pub fn gen_asm(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        self.emit("section .text".to_string());
        self.emit("global _start".to_string());
        self.emit("_start:".to_string());
        self.emit("    sub rsp, 8".to_string());
        self.emit("    call main".to_string());
        self.emit("    add rsp, 8".to_string());
        self.emit("    mov rax, 60".to_string());
        self.emit("    xor rdi, rdi".to_string());
        self.emit("    syscall".to_string());
        self.gen_stmts();
        self.emit("__bounds_fail__:".to_string());
        self.emit("    mov rax, 60".to_string());
        self.emit("    mov rdi, 1".to_string());
        self.emit("    syscall".to_string());
        Ok(self.m_out.clone())
    }

    pub fn lookup_var(&self, name: &str) -> &VarData {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return ty;
            }
        }
        self::panic!("couldnt find the var with name: {}", name);
    }

    pub fn add_var(&mut self, var_data: VarData, name: String) {
        let last_scope = self.scopes.last_mut().unwrap();
        last_scope.insert(name, var_data);
    }

    fn gen_stmts(&mut self) {
        let stmt = std::mem::take(&mut self.stmts);

        for i in stmt.iter() {
            match i {
                Stmt::InitFunc {
                    name,
                    args,
                    ret_type,
                    data,
                } => {
                    let func_data = FuncData {
                        args: args.clone(),
                        return_type: ret_type.clone(),
                    };
                    self.functions.insert(name.clone(), func_data);
                }
                Stmt::InitStruct(data) => {
                    self.gen_init_struct(data);
                }
                _ => {}
            }
        }

        for i in stmt.iter() {
            self.gen_stmt(i);
        }
    }
}

impl Stmt {
    pub fn get_type(&self, helper: &mut Analyzer) -> Option<(Type)> {
        match self {
            Stmt::Declaration(data) => {
                return Some(data.ty.clone());
            }
            Stmt::Assignment { target, value } => {
                let var = lvalue_root(target);
                if let Some(var) = helper.lookup(&var) {
                    return Some(var);
                } else {
                    helper.errors.push(SemanticError::UndeclaredVariable(var));
                    return None;
                }
            }
            Stmt::ExprStmt(expr) => {
                return Some(helper.check_expr(expr));
            }
            _ => None,
        }
    }
}
