use core::panic;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    dbg,
    env::var,
    fmt::{self, write},
    write,
};

use indexmap::IndexMap;

use crate::{
    Ir::{
        Stmt,
        expr::{Expr, ExprType},
        r#gen::{FuncData, StructData},
        sem_analysis::*,
        shared::TypeContext,
        stmt::{EnumData, EnumVariant, StmtType, StructField, Type},
    },
    shared::{check_types, is_number, substitute_type, type_name},
    tokenizer::TokenType,
};

pub mod sem_expr;
mod sem_stmt;

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SemanticError::EmptyArray => write!(
                f,
                "Cannot initialize an empty array without a specific type."
            ),
            SemanticError::UndeclaredVariable(name) => {
                write!(f, "Cannot find variable '{}' in this scope.", name)
            }
            SemanticError::UndeclaredFunction(name) => {
                write!(f, "Cannot find function '{}' in this scope.", name)
            }
            SemanticError::UndeclaredStruct(name) => {
                write!(f, "Cannot find struct '{}' in this scope.", name)
            }
            SemanticError::UndeclaredField(struct_name, field_name) => write!(
                f,
                "Struct '{}' has no field named '{}'.",
                struct_name, field_name
            ),
            SemanticError::AlreadyDeclared(name) => {
                write!(f, "The name '{}' is already defined in this scope.", name)
            }
            SemanticError::VoidVariable(name) => write!(
                f,
                "Variable '{}' cannot be declared with type 'void'.",
                name
            ),
            SemanticError::ArrayTooLarge {
                arr_name,
                expected,
                got,
            } => write!(
                f,
                "Array '{}' expects {} elements, but got {}.",
                arr_name, expected, got
            ),
            SemanticError::TypeMismatch { expected, got } => write!(
                f,
                "Type mismatch: expected {:?}, found {:?}.",
                expected, got
            ),
            SemanticError::StructCountMismatch {
                struct_name,
                expected,
                got,
            } => write!(
                f,
                "Struct '{}' expects {} fields, but got {}.",
                struct_name, expected, got
            ),
            SemanticError::StructTypeMismatch {
                struct_name,
                expected,
                got,
            } => write!(
                f,
                "Type mismatch in struct '{}' initialization: expected {:?}, found {:?}.",
                struct_name, expected, got
            ),
            SemanticError::StructNameNotFound { struct_name, got } => write!(
                f,
                "Invalid field '{}' provided when initializing struct '{}'.",
                got, struct_name
            ),
            SemanticError::ReturnTypeMismatch { expected, got } => write!(
                f,
                "Return type mismatch: expected {:?}, found {:?}.",
                expected, got
            ),
            SemanticError::NotAPointer(ty) => write!(
                f,
                "Type {:?} cannot be dereferenced. It is not a pointer.",
                ty
            ),
            SemanticError::NotIndexable(ty) => {
                write!(f, "Type {:?} is not an array and cannot be indexed.", ty)
            }
            SemanticError::NotAStruct(ty) => write!(
                f,
                "Type {:?} is not a struct and has no fields to access.",
                ty
            ),
            SemanticError::InvalidArrayIndex(ty) => write!(
                f,
                "Cannot index an array with type {:?}. Expected an integer.",
                ty
            ),
            SemanticError::NonArrayIndex(ty) => write!(f, "Type {:?} cannot be indexed.", ty),
            SemanticError::MatchTypeMismatch { expected, got } => write!(
                f,
                "Match arms have incompatible types: expected {:?}, found {:?}.",
                expected, got
            ),
            SemanticError::InvalidUnary { op, ty } => {
                write!(f, "Cannot apply unary operator {:?} to type {:?}.", op, ty)
            }
            SemanticError::InvalidBinary { op, left, right } => write!(
                f,
                "Cannot apply binary operator {:?} to types {:?} and {:?}.",
                op, left, right
            ),
            SemanticError::CastError { before, after } => write!(
                f,
                "Invalid cast: cannot cast from type {:?} to {:?}.",
                before, after
            ),
            SemanticError::MatchExprUnsuported(ty) => {
                write!(f, "Cannot match on type {:?}. Not supported.", ty)
            }
            _ => {
                write!(f, "{:?}", self)
            }
        }
    }
}

impl<'a> TypeContext for Analyzer<'a> {
    fn resolve_call(
        &self,
        name: &String,
        args: &Vec<Expr>,
        generics: &Vec<Type>,
    ) -> Option<(FuncData, usize)> {
        if generics.len() > 0 {
            let vec_func_data = self.get_function(name);
            if vec_func_data.len() < 1 {
                return None;
            }
            return Some((vec_func_data[0].clone(), 0));
        }

        let vec_func_data = self.get_function(name);
        if vec_func_data.len() < 1 {
            return None;
        }
        let (overload_pos, func_data) = vec_func_data
            .iter()
            .enumerate()
            .find(|(_, func)| {
                if func.args.len() != args.len() {
                    return false;
                }
                args.iter().enumerate().all(|(i, expr)| {
                    let expr_ty = expr.get_type(self);
                    let param_ty = &func.args[i].ty.clone();
                    let arg_matches = match &expr.ty {
                        ExprType::Number(_) => is_number(&param_ty),
                        _ => check_types(&expr_ty, &param_ty),
                    };
                    arg_matches
                })
            })
            .expect(&format!("no matching overload for function '{}'", name));

        Some((func_data.clone(), overload_pos))
    }

    fn monomorphize_struct(&self, def: &StructData, type_args: &Vec<Type>) -> Type {
        let mangled = format!(
            "{}__{}",
            def.name,
            type_args
                .iter()
                .map(|t| type_name(t))
                .collect::<Vec<_>>()
                .join("_")
        );
        if self.structs.borrow().contains_key(&mangled) {
            return Type::Struct(mangled.clone()); // already done
        }

        // substitute types in fields and recompute offsets
        let mut offset = 0;
        let fields: Vec<StructField> = def
            .elements
            .iter()
            .map(|f| {
                let concrete_ty = substitute_type(&f.1.ty, &def.generic_type, type_args);
                let field_size = self.type_size(&concrete_ty);
                let field = StructField {
                    name: f.1.name.clone(),
                    ty: concrete_ty,
                    offset,
                };
                offset += field_size;
                field
            })
            .collect();

        let size = self.compute_struct_size(&fields);
        self.structs.borrow_mut().insert(
            mangled.clone(),
            StructData {
                generic_type: Vec::new(),
                name: mangled.clone(),
                elements: fields.iter().map(|f| (f.name.clone(), f.clone())).collect(),
                size, // total size
            },
        );
        return Type::Struct(mangled);
    }

    fn field_alignment(&self, ty: &Type) -> usize {
        match ty {
            Type::Struct(name) => {
                let s = self.structs.borrow().get(name).cloned().unwrap();
                s.elements
                    .values()
                    .map(|f| self.field_alignment(&f.ty))
                    .max()
                    .unwrap_or(1)
            }
            Type::Enum(name, _) => {
                let e = self.enums.borrow().get(name).cloned().unwrap();
                let variant_align = e
                    .variants
                    .values()
                    .flat_map(|v| v.args.iter())
                    .map(|f| self.field_alignment(&f.ty))
                    .max()
                    .unwrap_or(1);
                8usize.max(variant_align)
            }
            _ => self.type_size(ty), // primitives: size == alignment
        }
    }

    fn monomorphize_enum(&self, def: &EnumData, type_args: &Vec<Type>) -> Type {
        let mangled = format!(
            "{}__{}",
            def.name,
            type_args
                .iter()
                .map(|t| type_name(t))
                .collect::<Vec<_>>()
                .join("_")
        );

        if self.enums.borrow().contains_key(&mangled) {
            return Type::Enum(mangled.clone(), None); // already done
        }

        let mut new_variants = HashMap::new();
        for (var_name, variant) in def.variants.iter() {
            let new_args: Vec<StructField> = variant
                .args
                .iter()
                .map(|arg| StructField {
                    name: arg.name.clone(),
                    ty: substitute_type(&arg.ty, &def.generic_type, type_args),
                    // the tag offest
                    offset: arg.offset + 8,
                })
                .collect();
            new_variants.insert(
                var_name.clone(),
                EnumVariant {
                    name: variant.name.clone(),
                    tag: variant.tag,
                    args: new_args,
                    size: 0,
                },
            );
        }
        self.enums.borrow_mut().insert(
            mangled.clone(),
            EnumData {
                name: mangled.clone(),
                generic_type: Vec::new(),
                variants: new_variants,
                size: 0,
            },
        );
        return Type::Enum(mangled, None);
    }

    fn ensure_monomorphized(&self, ty: &Type) -> Type {
        match ty {
            Type::GenericInst(name, type_args) => {
                let mangled = type_name(ty);
                if self.structs.borrow().contains_key(&mangled) {
                    return Type::Struct(mangled.clone());
                }
                if self.enums.borrow().contains_key(&mangled) {
                    return Type::Enum(mangled.clone(), None);
                }

                let struct_def = self.structs.borrow().get(name).cloned();
                let enum_def = self.enums.borrow().get(name).cloned();

                if let Some(struct_def) = struct_def {
                    return self.monomorphize_struct(&struct_def, type_args);
                } else if let Some(enum_def) = enum_def {
                    return self.monomorphize_enum(&enum_def, type_args);
                } else {
                    panic!("unknown generic type: {}", name);
                }
            }
            Type::Pointer(inner) => {
                let ty = self.ensure_monomorphized(inner);
                Type::Pointer(Box::new(ty))
            }
            Type::Array(inner, size) => {
                let ty = self.ensure_monomorphized(inner);
                Type::Array(Box::new(ty), *size)
            }
            _ => ty.clone(),
        }
    }
}

impl<'a> Analyzer<'a> {
    pub fn new(stmts: &'a Vec<Stmt>) -> Self {
        Self {
            stmts,
            generics: RefCell::new(HashMap::new()),
            had_error: Cell::new(false),
            scopes: vec![HashMap::new()], // start with global scope
            functions: HashMap::new(),
            break_stack: Vec::new(),
            contniue_stack: Vec::new(),
            structs: RefCell::new(HashMap::new()),
            current_file: String::new(),
            line: 0,
            col: 0,
            global_vars: HashMap::new(),
            enums: RefCell::new(HashMap::new()),
            generic_func: HashMap::new(),
            current_ret_type: Type::Unknown,
        }
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

    // this is just copy from gen
    // TODO: make this a trait so and expand it for gen and analyzer
    pub fn type_size(&self, ty: &Type) -> usize {
        match ty {
            Type::Primitive(token) => match token {
                TokenType::I8 | TokenType::U8 => 1,
                TokenType::I16 | TokenType::U16 => 2,
                TokenType::I32 | TokenType::U32 => 4,
                TokenType::I64 | TokenType::U64 => 8,
                _ => panic!("Unsupported primitive type: {:?}", token),
            },
            Type::Pointer(_) => 8,
            Type::Array(elem_type, count) => self.type_size(elem_type),
            Type::Struct(name) => {
                self.structs
                    .borrow()
                    .get(name)
                    .expect(&format!("Unknown struct: {}", name))
                    .size
            }
            Type::GenericInst(str, ty) => panic!("generic inst isnt monomorphized"),
            Type::GenericType(name) => {
                // TODO: make the self.generic the same as in gen and fix this
                8
            }
            Type::Enum(..) => 8,
            Type::Unknown => panic!("unkown type"),
        }
    }

    pub fn print_error(&self, err: Error) {
        eprintln!("\x1b[31;1merror\x1b[0m: \x1b[1m{}\x1b[0m", err.ty);

        eprintln!(
            "  \x1b[34;1m-->\x1b[0m {}:{}:{}",
            err.file, err.line, err.col
        );

        self.had_error.set(true);
    }

    pub fn check_init(&mut self, stmt: &Stmt) {
        self.current_file = stmt.file.clone();
        self.line = stmt.line;
        match &stmt.ty {
            StmtType::GenericInitFunc {
                name,
                generic_types,
                args,
                ret_type,
                data,
            } => {
                let func_data = FuncData {
                    args: args.clone(),
                    generic: generic_types.clone(),
                    return_type: ret_type.clone(),
                };
                self.functions
                    .entry(name.clone())
                    .or_insert_with(Vec::new)
                    .push(func_data);
                self.generic_func.insert(name.clone(), stmt.clone());
            }
            StmtType::InitFunc {
                name,
                args,
                ret_type,
                ..
            } => {
                let func_data = FuncData {
                    args: args.clone(),
                    generic: Vec::new(),
                    return_type: ret_type.clone(),
                };
                self.functions
                    .entry(name.clone())
                    .or_insert_with(Vec::new)
                    .push(func_data);
            }
            StmtType::ExternFn(data) => {
                self.check_init(data);
            }
            StmtType::InitStruct(data) => {
                let fields = data
                    .fields
                    .iter()
                    .map(|f| (f.name.clone(), f.clone()))
                    .collect::<IndexMap<_, _>>();

                let size = self.compute_struct_size(&data.fields);
                let struct_data = StructData {
                    name: data.name.clone(),
                    generic_type: data.generic_type.clone(),
                    size,
                    elements: fields,
                };
                self.structs
                    .borrow_mut()
                    .insert(data.name.clone(), struct_data);
            }
            StmtType::InitEnum {
                name,
                variants,
                generic_types,
            } => {
                let enum_data = EnumData {
                    name: name.clone(),
                    generic_type: generic_types.clone(),
                    variants: variants.clone(),
                    size: 0,
                };
                self.enums.borrow_mut().insert(name.clone(), enum_data);
            }
            _ => {}
        }
    }

    pub fn check_inits(&mut self) {
        for stmt in self.stmts.clone().iter() {
            self.check_init(stmt);
        }
    }

    pub fn type_to_error(&self, error_ty: SemanticError) -> Error {
        Error {
            ty: error_ty,
            file: self.current_file.clone(),
            line: self.line,
            col: self.col,
        }
    }

    pub fn lookup(&self, expected_name: &String) -> Option<Type> {
        for i in self.scopes.iter() {
            for (name, ty) in i {
                if name == expected_name {
                    return Some(ty.clone());
                }
            }
        }
        if let Some(global_data) = self.global_vars.get(expected_name) {
            return Some(global_data.clone());
        }
        return None;
    }

    pub fn get_function(&self, name: &String) -> Vec<FuncData> {
        let func_data = self.functions.get(name);
        if func_data.is_some() {
            return func_data.unwrap().to_vec();
        } else {
            self.print_error(self.type_to_error(SemanticError::UndeclaredFunction(name.clone())));
            Vec::new()
        }
    }

    pub fn add_var(&mut self, name: String, ty: Type) {
        let map = self.scopes.last_mut().unwrap();
        map.insert(name, ty);
    }

    pub fn check_code(&mut self) {
        //first iteration to get all structs and func data
        self.check_inits();
        // checking of every stmt
        for i in self.stmts.iter() {
            self.check_stmt(i);
        }
    }
}
