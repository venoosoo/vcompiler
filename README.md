# vcompiler

A compiler for a custom C-like language, written from scratch in Rust. Compiles `.v` source files directly to x86-64 NASM assembly, which can be assembled and linked into a native Linux executable.

> **Status:** Work in progress. Core pipeline is functional — see [what's working](#whats-working) below.

---

## What it does

Takes `.v` source code like this:

```c
fn print_num(long n) {
    if n < 0 {
        asm {
            "sub rsp, 1"
            "mov byte [rsp], '-'"
            "mov rax, 1"
            "mov rdi, 1"
            "mov rsi, rsp"
            "mov rdx, 1"
            "syscall"
            "add rsp, 1"
        }
        n = -n;
    }
    if n >= 10 {
        long temp = n / 10;
        print_num(temp);
    }
    char c = (n % 10) + '0';
    asm {
        "mov rax, 1"
        "mov rdi, 1"
        "lea rsi, (c)"
        "mov rdx, 1"
        "syscall"
    }
}
```

And produces real x86-64 NASM assembly:

```nasm
print_num:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov [rbp - 8], rdi
    mov rax, 0
    push rax
    mov rax, QWORD [rbp - 8]
    pop rbx
    cmp rax, rbx
    setl al
    movzx rax, al
    cmp rax, 0
    je end_if_1
if_1:
    sub rsp, 1
    mov byte [rsp], '-'
    ...
```

---

## Pipeline

```
.v source file
    │
    ▼
 Tokenizer        — lexes keywords, operators, literals, identifiers
    │
    ▼
  Parser          — recursive descent, Pratt-style expression parsing
    │
    ▼
   IR / AST       — typed statement and expression tree
    │
    ▼
Semantic Analysis — type checking, scope resolution, function signatures
    │
    ▼
 Code Generator   — stack-machine expression eval, x86-64 NASM output
    │
    ▼
  main.asm        — assemble with nasm + ld
```

---

## What's working

- ✅ Tokenizer — full lexer for all language tokens
- ✅ Parser — recursive descent with Pratt-style expression parsing
- ✅ AST / IR — typed expression and statement nodes
- ✅ Semantic analysis — type checking, scope resolution, function/struct registration
- ✅ Code generation — x86-64 NASM output, System V ABI compliant
- ✅ Primitive types: `int`, `char`, `short`, `long`, `void`
- ✅ Pointers and dereferencing (`*`, `&`)
- ✅ Arrays with bounds-checked index access
- ✅ Structs with `.` (stack) and `->` (pointer) field access
- ✅ String literals — `char[] name = "hello"` with automatic null termination and size inference
- ✅ Functions with typed arguments and return values
- ✅ Control flow: `if`/`else`, `while`, `for`
- ✅ Inline `asm {}` blocks with variable substitution via `(varname)`
- ✅ Arithmetic, comparison, logical operators
- ✅ Recursion
- ✅ Global variables
- ✅ File imports via `import "file.v"`
- ✅ `sizeof(Type)` operator
- ✅ Standard library: `print`, `print_str`, `print_char`, `strlen`, `malloc`, `free`, `memcpy`, `syscall`, `exit`
- ✅ Generic `Vector` type with `create_vector`, `vector_push`, `vector_pop`, `vec_get_element`

## In progress / planned

- 🔧 Full semantic analysis rewrite — proper numeric coercion, array-to-pointer decay
- 🔧 Macro system
- 🔧 Improved error reporting with source locations

---

## Standard library

The standard library lives in `std/` and is imported with `import "std/std.v"`.

```c
// Printing
fn print(long number)                          // print integer + newline
fn print_char(char t)                          // print single character
fn print_str(char* str, long length)           // print string + newline
fn strlen(char* str) -> long                   // string length

// Memory
fn malloc(long size) -> void*                  // heap allocate (mmap-backed)
fn free(long* ptr, long size)                  // free heap memory
fn memcpy(void* dst, void* src, long size)     // copy bytes

// System
fn syscall(long rax, long rdi, long rsi,
           long rdx, long r10, long r8) -> void*
fn exit(int code)

// Vector (std/vector.v)
fn create_vector(long element_size) -> Vector*
fn vector_push(Vector* vec, void* element)
fn vector_pop(Vector* vec) -> void*
fn vec_get_element(Vector* vec, int pos) -> void*
```

---

## Language syntax

```c
// Functions
fn add(int a, int b) -> int {
    return a + b;
}

// Variables and types
int x = 42;
char c = 'A';
long big = 1000000;
void* ptr;

// Pointers
int* p = &x;
int val = *p;

// Arrays
int arr[10];
arr[0] = 1;

// String literals (size inferred automatically)
char[] name = "hello";
char greeting[8] = "jackpot";

// Structs
struct Point {
    int x;
    int y;
}
Point p = Point { x: 1, y: 2 };   // stack allocated, use .
Point* hp = malloc(sizeof(Point)); // heap allocated, use ->
hp->x = 10;

// Control flow
if x > 10 {
    // ...
} else {
    // ...
}

while x > 0 {
    x = x - 1;
}

for (int i = 0; i < 10; i = i + 1) {
    // ...
}

// Globals
global int counter;

// Inline assembly (variables substituted via (varname))
asm {
    "lea rsi, (c)"
    "mov rdx, 1"
    "syscall"
}

// Imports
import "std/std.v"
import "std/vector.v"
```

---

## Build & run

**Requirements:** Rust, NASM, ld (Linux only)

```bash
# Build the compiler
cargo build --release

# Compile a .v file
./target/release/vcompiler --file your_program.v

# Assemble and link the output
nasm -f elf64 main.asm -o main.o
ld main.o -o main

# Run
./main
```

---

## Project structure

```
src/
├── main.rs                  — CLI entry point
├── Tokenizer/mod.rs         — lexer
├── Parser/
│   ├── expr.rs              — expression parsing (Pratt precedence climbing)
│   ├── stmt.rs              — statement parsing
│   └── function.rs          — function definition parsing
├── Ir/
│   ├── stmt.rs              — statement, type, and LValue definitions
│   ├── expr.rs              — expression definitions
│   ├── gen.rs               — VarData, FuncData, StructData, Addr types
│   └── sem_analysis.rs      — semantic error types and Analyzer struct
├── sem_analysis/
│   ├── mod.rs               — Analyzer impl, scope management, check_code
│   ├── sem_expr.rs          — expression type checking
│   └── sem_stmt.rs          — statement type checking
└── Gen/
    ├── mod.rs               — Gen struct, reg_for_size, arg_pos, helpers
    ├── gen_expr.rs          — expression codegen (stack-machine eval)
    └── gen_stmt.rs          — statement codegen, lvalue resolution

std/
├── std.v                    — print, malloc, memcpy, syscall, strlen, exit
└── vector.v                 — generic Vector with push/pop/get
```

---

## Why

Because i can
