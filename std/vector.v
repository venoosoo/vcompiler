import "std.v"

struct Vector<T> {
    T* data;
    long length;
    long capacity;
    long element_size;
}

fn create_vector<T>() -> *Vector<T> {
    int el_size = sizeof(T);
    long base_capacity = 8;
    long* memory = malloc(el_size * base_capacity);
    Vector<T>* res = malloc(sizeof(Vector<T>));
    res->data = memory;
    res->length = 0;
    res->capacity = base_capacity;
    res->element_size = el_size;
    return res;
}

fn vector_push<T>(Vector<T>* vec, T* element) {
    if vec->length == vec->capacity {
        long new_capacity = vec->capacity * 2;
        T* new_data = malloc(new_capacity * vec->element_size);
        memcpy(new_data, vec->data, vec->length * vec->element_size);
        vec->data = new_data;
        vec->capacity = new_capacity;
    }
    long offset = vec->length * vec->element_size;
    T* dest = vec->data + offset;
    memcpy(dest, element, vec->element_size);
    vec->length = vec->length + 1;
}

fn vector_pop<T>(Vector<T>* vec) -> *T {
    if vec->length == 0 {
        exit(1);
    }
    vec->length = vec->length - 1;
    long offset = vec->length * vec->element_size;
    int* element = vec->data + offset;
    return *element;
}

fn vec_get_element<T>(Vector<T>* vec, int element_pos) -> *T {
    if vec->length < element_pos {
        exit(1);
    }
    long offset = vec->element_size * element_pos;
    int* element = vec->data + offset;
    return *element;
}