fn print_num(long n) {
    long rem;
    char c;
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
    rem = n % 10;
    c = rem + '0';
    asm {
        "mov rax, 1"
        "mov rdi, 1"
        "lea rsi, (c)"
        "mov rdx, 1"
        "syscall"
    }
}

fn print(char* str) {
    int length = 0;
    while str[length] != 0 {
        print_char(str[length])
        length = length + 1;
    }
    asm {
        "sub rsp, 1"
        "mov byte [rsp], 10"
        "mov rax, 1"
        "mov rdi, 1"
        "mov rsi, rsp"
        "mov rdx, 1"
        "syscall"
        "add rsp, 1"
    }
}


fn print(long number) {
    print_num(number);
    asm {
        "sub rsp, 1"
        "mov byte [rsp], 10"
        "mov rax, 1"
        "mov rdi, 1"
        "mov rsi, rsp"
        "mov rdx, 1"
        "syscall"
        "add rsp, 1"
    }
}

fn exit(int code) {
    asm {
        "mov rax, 60"
        "mov rdi, (code)"
        "syscall"
    }
}




global long* malloc_heap_start;
global long* malloc_heap_current;
global long malloc_heap_remaining;

fn malloc(long size) -> void* {
    if malloc_heap_start == 0 {
        malloc_heap_start = syscall(9,0,4096,3,34,-1);
        malloc_heap_current = malloc_heap_start;
        malloc_heap_remaining = 4096;
    }
    if malloc_heap_remaining < size {
        malloc_heap_start = syscall(9,0,4096,3,34,-1);
        malloc_heap_current = malloc_heap_start;
        malloc_heap_remaining = 4096;
    }
    long* ptr = malloc_heap_current;
    malloc_heap_current = malloc_heap_current + size;
    malloc_heap_remaining = malloc_heap_remaining - size;
    return ptr
}

fn free(long* ptr, long size) {
    syscall(11, ptr as long, size, 0, 0, 0);
}

fn memcpy(void* dst, void* src, long size) -> void {
    asm {
        "mov rcx, rdx"
        "rep movsb"
    }
}


fn syscall(long a_rax, long a_rdi, long a_rsi, long a_rdx, long a_r10, long a_r8) -> void* {
    asm {
        "mov rax, [rbp - 8]"
        "mov rdi, [rbp - 16]"
        "mov rsi, [rbp - 24]"
        "mov rdx, [rbp - 32]"
        "mov r10, [rbp - 40]"
        "mov r8,  [rbp - 48]"
        "mov r9, 0"
        "syscall"
    }
    return;
}


fn strlen(char* str) -> long {
    long i = 0;
    while str[i] != 0 {
        i = i + 1;
    }
    return i;
}

fn print_char(char t) {
    asm {
        "movsx rax, BYTE [rbp - 1]"
        "sub rsp, 1"
        "mov [rsp], al"
        "mov rax, 1"
        "mov rdi, 1"
        "mov rsi, rsp"
        "mov rdx, 1"
        "syscall"
        "add rsp, 1"
    }
}


enum Option<T> {
    Some{
        T data;
    }
    None,
}


fn unwrap<T>(Option<T>* data) -> T {
    match *data {
        Option::Some(data2) => {
            return data2
        }
        Option::None => {
            print("unwrap at none value");
            exit(1);
        }
    }
    exit(1);
}