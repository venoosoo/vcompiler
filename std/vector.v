import "std.v"


struct Vector {
    void* data;
    long length;
    long capacity;
    long element_size;
}



fn create_vector(long element_size) -> Vector* {
    long base_capacity = 32;
    long* memory = malloc(element_size * base_capacity);
    Vector* res = malloc(sizeof(Vector));
    res->data = memory;
    res->length = 0;
    res->capacity = base_capacity;
    res->element_size = element_size;
    return res;
}


fn vector_push(Vector* vec, void* element) {
    if vec->length == vec->capacity {
        long new_capacity = vec->capacity * 2;
        void* new_data = malloc(new_capacity * vec->element_size);
        memcpy(new_data, vec->data, vec->length * vec->element_size);
        vec->data = new_data;
        vec->capacity = new_capacity;
    }
    long offset = vec->length * vec->element_size;
    void* dest = vec->data + offset;
    memcpy(dest,element, vec->element_size);
    vec->length = vec->length + 1;

}

fn vector_pop(Vector* vec) -> void* {
    if vec->length == 0 {
        exit(1);
    }
    vec->length = vec->length - 1;
    long offset = vec->length * vec->element_size;
    return vec->data + offset;
}

fn vec_get_element(Vector* vec, int element_pos) -> void* {
    if vec->length < element_pos {
        exit(1);
    }
    long offest = vec->element_size * element_pos;
    return vec->data + offest;
}




