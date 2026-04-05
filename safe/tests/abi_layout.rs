use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::mem::{align_of, offset_of, size_of};
use std::path::{Path, PathBuf};
use std::process::Command;

use lzma::ffi::types::*;

fn cargo_manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build_dir() -> PathBuf {
    cargo_manifest_dir().join("target/abi-layout")
}

fn compile_c_probe(source: &Path, output: &Path) {
    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let include_dir = cargo_manifest_dir().join("include");

    let status = Command::new(cc)
        .arg("-std=gnu11")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-I")
        .arg(include_dir)
        .arg(source)
        .arg("-o")
        .arg(output)
        .status()
        .expect("failed to invoke C compiler");

    assert!(status.success(), "failed to compile ABI probe");
}

fn c_probe_source() -> String {
    r#"
#include <stddef.h>
#include <stdio.h>
#include <lzma.h>

#define PRINT_STRUCT(type, name) \
    printf("struct\t%s\t%zu\t%zu\n", name, sizeof(type), _Alignof(type))
#define PRINT_FIELD(type, field) \
    printf("field\t" #type "\t" #field "\t%zu\n", offsetof(type, field))

int main(void) {
    PRINT_STRUCT(lzma_allocator, "lzma_allocator");
    PRINT_FIELD(lzma_allocator, alloc);
    PRINT_FIELD(lzma_allocator, free);
    PRINT_FIELD(lzma_allocator, opaque);

    PRINT_STRUCT(lzma_stream, "lzma_stream");
    PRINT_FIELD(lzma_stream, next_in);
    PRINT_FIELD(lzma_stream, avail_in);
    PRINT_FIELD(lzma_stream, total_in);
    PRINT_FIELD(lzma_stream, next_out);
    PRINT_FIELD(lzma_stream, avail_out);
    PRINT_FIELD(lzma_stream, total_out);
    PRINT_FIELD(lzma_stream, allocator);
    PRINT_FIELD(lzma_stream, internal);
    PRINT_FIELD(lzma_stream, reserved_ptr1);
    PRINT_FIELD(lzma_stream, reserved_ptr2);
    PRINT_FIELD(lzma_stream, reserved_ptr3);
    PRINT_FIELD(lzma_stream, reserved_ptr4);
    PRINT_FIELD(lzma_stream, seek_pos);
    PRINT_FIELD(lzma_stream, reserved_int2);
    PRINT_FIELD(lzma_stream, reserved_int3);
    PRINT_FIELD(lzma_stream, reserved_int4);
    PRINT_FIELD(lzma_stream, reserved_enum1);
    PRINT_FIELD(lzma_stream, reserved_enum2);

    PRINT_STRUCT(lzma_mt, "lzma_mt");
    PRINT_FIELD(lzma_mt, flags);
    PRINT_FIELD(lzma_mt, threads);
    PRINT_FIELD(lzma_mt, block_size);
    PRINT_FIELD(lzma_mt, timeout);
    PRINT_FIELD(lzma_mt, preset);
    PRINT_FIELD(lzma_mt, filters);
    PRINT_FIELD(lzma_mt, check);
    PRINT_FIELD(lzma_mt, reserved_enum1);
    PRINT_FIELD(lzma_mt, reserved_enum2);
    PRINT_FIELD(lzma_mt, reserved_enum3);
    PRINT_FIELD(lzma_mt, reserved_int1);
    PRINT_FIELD(lzma_mt, reserved_int2);
    PRINT_FIELD(lzma_mt, reserved_int3);
    PRINT_FIELD(lzma_mt, reserved_int4);
    PRINT_FIELD(lzma_mt, memlimit_threading);
    PRINT_FIELD(lzma_mt, memlimit_stop);
    PRINT_FIELD(lzma_mt, reserved_int7);
    PRINT_FIELD(lzma_mt, reserved_int8);
    PRINT_FIELD(lzma_mt, reserved_ptr1);
    PRINT_FIELD(lzma_mt, reserved_ptr2);
    PRINT_FIELD(lzma_mt, reserved_ptr3);
    PRINT_FIELD(lzma_mt, reserved_ptr4);

    PRINT_STRUCT(lzma_filter, "lzma_filter");
    PRINT_FIELD(lzma_filter, id);
    PRINT_FIELD(lzma_filter, options);

    PRINT_STRUCT(lzma_block, "lzma_block");
    PRINT_FIELD(lzma_block, version);
    PRINT_FIELD(lzma_block, header_size);
    PRINT_FIELD(lzma_block, check);
    PRINT_FIELD(lzma_block, compressed_size);
    PRINT_FIELD(lzma_block, uncompressed_size);
    PRINT_FIELD(lzma_block, filters);
    PRINT_FIELD(lzma_block, raw_check);
    PRINT_FIELD(lzma_block, reserved_ptr1);
    PRINT_FIELD(lzma_block, reserved_ptr2);
    PRINT_FIELD(lzma_block, reserved_ptr3);
    PRINT_FIELD(lzma_block, reserved_int1);
    PRINT_FIELD(lzma_block, reserved_int2);
    PRINT_FIELD(lzma_block, reserved_int3);
    PRINT_FIELD(lzma_block, reserved_int4);
    PRINT_FIELD(lzma_block, reserved_int5);
    PRINT_FIELD(lzma_block, reserved_int6);
    PRINT_FIELD(lzma_block, reserved_int7);
    PRINT_FIELD(lzma_block, reserved_int8);
    PRINT_FIELD(lzma_block, reserved_enum1);
    PRINT_FIELD(lzma_block, reserved_enum2);
    PRINT_FIELD(lzma_block, reserved_enum3);
    PRINT_FIELD(lzma_block, reserved_enum4);
    PRINT_FIELD(lzma_block, ignore_check);
    PRINT_FIELD(lzma_block, reserved_bool2);
    PRINT_FIELD(lzma_block, reserved_bool3);
    PRINT_FIELD(lzma_block, reserved_bool4);
    PRINT_FIELD(lzma_block, reserved_bool5);
    PRINT_FIELD(lzma_block, reserved_bool6);
    PRINT_FIELD(lzma_block, reserved_bool7);
    PRINT_FIELD(lzma_block, reserved_bool8);

    PRINT_STRUCT(lzma_stream_flags, "lzma_stream_flags");
    PRINT_FIELD(lzma_stream_flags, version);
    PRINT_FIELD(lzma_stream_flags, backward_size);
    PRINT_FIELD(lzma_stream_flags, check);
    PRINT_FIELD(lzma_stream_flags, reserved_enum1);
    PRINT_FIELD(lzma_stream_flags, reserved_enum2);
    PRINT_FIELD(lzma_stream_flags, reserved_enum3);
    PRINT_FIELD(lzma_stream_flags, reserved_enum4);
    PRINT_FIELD(lzma_stream_flags, reserved_bool1);
    PRINT_FIELD(lzma_stream_flags, reserved_bool2);
    PRINT_FIELD(lzma_stream_flags, reserved_bool3);
    PRINT_FIELD(lzma_stream_flags, reserved_bool4);
    PRINT_FIELD(lzma_stream_flags, reserved_bool5);
    PRINT_FIELD(lzma_stream_flags, reserved_bool6);
    PRINT_FIELD(lzma_stream_flags, reserved_bool7);
    PRINT_FIELD(lzma_stream_flags, reserved_bool8);
    PRINT_FIELD(lzma_stream_flags, reserved_int1);
    PRINT_FIELD(lzma_stream_flags, reserved_int2);

    PRINT_STRUCT(lzma_options_lzma, "lzma_options_lzma");
    PRINT_FIELD(lzma_options_lzma, dict_size);
    PRINT_FIELD(lzma_options_lzma, preset_dict);
    PRINT_FIELD(lzma_options_lzma, preset_dict_size);
    PRINT_FIELD(lzma_options_lzma, lc);
    PRINT_FIELD(lzma_options_lzma, lp);
    PRINT_FIELD(lzma_options_lzma, pb);
    PRINT_FIELD(lzma_options_lzma, mode);
    PRINT_FIELD(lzma_options_lzma, nice_len);
    PRINT_FIELD(lzma_options_lzma, mf);
    PRINT_FIELD(lzma_options_lzma, depth);
    PRINT_FIELD(lzma_options_lzma, ext_flags);
    PRINT_FIELD(lzma_options_lzma, ext_size_low);
    PRINT_FIELD(lzma_options_lzma, ext_size_high);
    PRINT_FIELD(lzma_options_lzma, reserved_int4);
    PRINT_FIELD(lzma_options_lzma, reserved_int5);
    PRINT_FIELD(lzma_options_lzma, reserved_int6);
    PRINT_FIELD(lzma_options_lzma, reserved_int7);
    PRINT_FIELD(lzma_options_lzma, reserved_int8);
    PRINT_FIELD(lzma_options_lzma, reserved_enum1);
    PRINT_FIELD(lzma_options_lzma, reserved_enum2);
    PRINT_FIELD(lzma_options_lzma, reserved_enum3);
    PRINT_FIELD(lzma_options_lzma, reserved_enum4);
    PRINT_FIELD(lzma_options_lzma, reserved_ptr1);
    PRINT_FIELD(lzma_options_lzma, reserved_ptr2);

    PRINT_STRUCT(lzma_options_delta, "lzma_options_delta");
    PRINT_FIELD(lzma_options_delta, type);
    PRINT_FIELD(lzma_options_delta, dist);
    PRINT_FIELD(lzma_options_delta, reserved_int1);
    PRINT_FIELD(lzma_options_delta, reserved_int2);
    PRINT_FIELD(lzma_options_delta, reserved_int3);
    PRINT_FIELD(lzma_options_delta, reserved_int4);
    PRINT_FIELD(lzma_options_delta, reserved_ptr1);
    PRINT_FIELD(lzma_options_delta, reserved_ptr2);

    PRINT_STRUCT(lzma_options_bcj, "lzma_options_bcj");
    PRINT_FIELD(lzma_options_bcj, start_offset);

    PRINT_STRUCT(lzma_index_iter, "lzma_index_iter");
    PRINT_STRUCT(__typeof__(((lzma_index_iter *)0)->stream), "lzma_index_iter.stream");
    PRINT_STRUCT(__typeof__(((lzma_index_iter *)0)->block), "lzma_index_iter.block");
    PRINT_STRUCT(__typeof__(((lzma_index_iter *)0)->internal[0]), "lzma_index_iter.internal[0]");
    PRINT_FIELD(lzma_index_iter, stream);
    PRINT_FIELD(lzma_index_iter, stream.flags);
    PRINT_FIELD(lzma_index_iter, stream.reserved_ptr1);
    PRINT_FIELD(lzma_index_iter, stream.reserved_ptr2);
    PRINT_FIELD(lzma_index_iter, stream.reserved_ptr3);
    PRINT_FIELD(lzma_index_iter, stream.number);
    PRINT_FIELD(lzma_index_iter, stream.block_count);
    PRINT_FIELD(lzma_index_iter, stream.compressed_offset);
    PRINT_FIELD(lzma_index_iter, stream.uncompressed_offset);
    PRINT_FIELD(lzma_index_iter, stream.compressed_size);
    PRINT_FIELD(lzma_index_iter, stream.uncompressed_size);
    PRINT_FIELD(lzma_index_iter, stream.padding);
    PRINT_FIELD(lzma_index_iter, stream.reserved_vli1);
    PRINT_FIELD(lzma_index_iter, stream.reserved_vli2);
    PRINT_FIELD(lzma_index_iter, stream.reserved_vli3);
    PRINT_FIELD(lzma_index_iter, stream.reserved_vli4);
    PRINT_FIELD(lzma_index_iter, block);
    PRINT_FIELD(lzma_index_iter, block.number_in_file);
    PRINT_FIELD(lzma_index_iter, block.compressed_file_offset);
    PRINT_FIELD(lzma_index_iter, block.uncompressed_file_offset);
    PRINT_FIELD(lzma_index_iter, block.number_in_stream);
    PRINT_FIELD(lzma_index_iter, block.compressed_stream_offset);
    PRINT_FIELD(lzma_index_iter, block.uncompressed_stream_offset);
    PRINT_FIELD(lzma_index_iter, block.uncompressed_size);
    PRINT_FIELD(lzma_index_iter, block.unpadded_size);
    PRINT_FIELD(lzma_index_iter, block.total_size);
    PRINT_FIELD(lzma_index_iter, block.reserved_vli1);
    PRINT_FIELD(lzma_index_iter, block.reserved_vli2);
    PRINT_FIELD(lzma_index_iter, block.reserved_vli3);
    PRINT_FIELD(lzma_index_iter, block.reserved_vli4);
    PRINT_FIELD(lzma_index_iter, block.reserved_ptr1);
    PRINT_FIELD(lzma_index_iter, block.reserved_ptr2);
    PRINT_FIELD(lzma_index_iter, block.reserved_ptr3);
    PRINT_FIELD(lzma_index_iter, block.reserved_ptr4);
    PRINT_FIELD(lzma_index_iter, internal);

    return 0;
}
"#
    .to_string()
}

fn run_probe() -> (
    BTreeMap<String, (usize, usize)>,
    BTreeMap<(String, String), usize>,
) {
    let build_dir = build_dir();
    let source_path = build_dir.join("abi_probe.c");
    let binary_path = build_dir.join("abi_probe");

    fs::create_dir_all(&build_dir).expect("failed to create ABI layout build directory");
    fs::write(&source_path, c_probe_source()).expect("failed to write ABI probe source");
    compile_c_probe(&source_path, &binary_path);

    let output = Command::new(&binary_path)
        .output()
        .expect("failed to execute ABI probe");

    assert!(output.status.success(), "ABI probe exited unsuccessfully");

    let stdout = String::from_utf8(output.stdout).expect("ABI probe output was not UTF-8");
    let mut structs = BTreeMap::new();
    let mut fields = BTreeMap::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.as_slice() {
            ["struct", name, size, align] => {
                structs.insert(
                    (*name).to_string(),
                    (
                        size.parse::<usize>().expect("invalid size"),
                        align.parse::<usize>().expect("invalid align"),
                    ),
                );
            }
            ["field", ty, field, offset] => {
                fields.insert(
                    ((*ty).to_string(), (*field).to_string()),
                    offset.parse::<usize>().expect("invalid field offset"),
                );
            }
            _ => panic!("unexpected ABI probe line: {line}"),
        }
    }

    (structs, fields)
}

fn assert_struct(
    structs: &BTreeMap<String, (usize, usize)>,
    name: &str,
    rust_size: usize,
    rust_align: usize,
) {
    let (c_size, c_align) = structs.get(name).copied().expect("missing C struct");
    assert_eq!(c_size, rust_size, "size mismatch for {name}");
    assert_eq!(c_align, rust_align, "alignment mismatch for {name}");
}

fn assert_field(
    fields: &BTreeMap<(String, String), usize>,
    ty: &str,
    field: &str,
    rust_offset: usize,
) {
    let key = (ty.to_string(), field.to_string());
    let c_offset = fields.get(&key).copied().expect("missing C field");
    assert_eq!(c_offset, rust_offset, "offset mismatch for {ty}.{field}");
}

#[test]
fn public_abi_layout_matches_upstream_headers() {
    let (structs, fields) = run_probe();

    assert_struct(
        &structs,
        "lzma_allocator",
        size_of::<lzma_allocator>(),
        align_of::<lzma_allocator>(),
    );
    assert_struct(
        &structs,
        "lzma_stream",
        size_of::<lzma_stream>(),
        align_of::<lzma_stream>(),
    );
    assert_struct(
        &structs,
        "lzma_mt",
        size_of::<lzma_mt>(),
        align_of::<lzma_mt>(),
    );
    assert_struct(
        &structs,
        "lzma_filter",
        size_of::<lzma_filter>(),
        align_of::<lzma_filter>(),
    );
    assert_struct(
        &structs,
        "lzma_block",
        size_of::<lzma_block>(),
        align_of::<lzma_block>(),
    );
    assert_struct(
        &structs,
        "lzma_stream_flags",
        size_of::<lzma_stream_flags>(),
        align_of::<lzma_stream_flags>(),
    );
    assert_struct(
        &structs,
        "lzma_options_lzma",
        size_of::<lzma_options_lzma>(),
        align_of::<lzma_options_lzma>(),
    );
    assert_struct(
        &structs,
        "lzma_options_delta",
        size_of::<lzma_options_delta>(),
        align_of::<lzma_options_delta>(),
    );
    assert_struct(
        &structs,
        "lzma_options_bcj",
        size_of::<lzma_options_bcj>(),
        align_of::<lzma_options_bcj>(),
    );
    assert_struct(
        &structs,
        "lzma_index_iter",
        size_of::<lzma_index_iter>(),
        align_of::<lzma_index_iter>(),
    );
    assert_struct(
        &structs,
        "lzma_index_iter.stream",
        size_of::<lzma_index_iter_stream>(),
        align_of::<lzma_index_iter_stream>(),
    );
    assert_struct(
        &structs,
        "lzma_index_iter.block",
        size_of::<lzma_index_iter_block>(),
        align_of::<lzma_index_iter_block>(),
    );
    assert_struct(
        &structs,
        "lzma_index_iter.internal[0]",
        size_of::<lzma_index_iter_internal>(),
        align_of::<lzma_index_iter_internal>(),
    );

    assert_field(
        &fields,
        "lzma_allocator",
        "alloc",
        offset_of!(lzma_allocator, alloc),
    );
    assert_field(
        &fields,
        "lzma_allocator",
        "free",
        offset_of!(lzma_allocator, free),
    );
    assert_field(
        &fields,
        "lzma_allocator",
        "opaque",
        offset_of!(lzma_allocator, opaque),
    );

    assert_field(
        &fields,
        "lzma_stream",
        "next_in",
        offset_of!(lzma_stream, next_in),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "avail_in",
        offset_of!(lzma_stream, avail_in),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "total_in",
        offset_of!(lzma_stream, total_in),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "next_out",
        offset_of!(lzma_stream, next_out),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "avail_out",
        offset_of!(lzma_stream, avail_out),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "total_out",
        offset_of!(lzma_stream, total_out),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "allocator",
        offset_of!(lzma_stream, allocator),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "internal",
        offset_of!(lzma_stream, internal),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_ptr1",
        offset_of!(lzma_stream, reserved_ptr1),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_ptr2",
        offset_of!(lzma_stream, reserved_ptr2),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_ptr3",
        offset_of!(lzma_stream, reserved_ptr3),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_ptr4",
        offset_of!(lzma_stream, reserved_ptr4),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "seek_pos",
        offset_of!(lzma_stream, seek_pos),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_int2",
        offset_of!(lzma_stream, reserved_int2),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_int3",
        offset_of!(lzma_stream, reserved_int3),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_int4",
        offset_of!(lzma_stream, reserved_int4),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_enum1",
        offset_of!(lzma_stream, reserved_enum1),
    );
    assert_field(
        &fields,
        "lzma_stream",
        "reserved_enum2",
        offset_of!(lzma_stream, reserved_enum2),
    );

    assert_field(&fields, "lzma_mt", "flags", offset_of!(lzma_mt, flags));
    assert_field(&fields, "lzma_mt", "threads", offset_of!(lzma_mt, threads));
    assert_field(
        &fields,
        "lzma_mt",
        "block_size",
        offset_of!(lzma_mt, block_size),
    );
    assert_field(&fields, "lzma_mt", "timeout", offset_of!(lzma_mt, timeout));
    assert_field(&fields, "lzma_mt", "preset", offset_of!(lzma_mt, preset));
    assert_field(&fields, "lzma_mt", "filters", offset_of!(lzma_mt, filters));
    assert_field(&fields, "lzma_mt", "check", offset_of!(lzma_mt, check));
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_enum1",
        offset_of!(lzma_mt, reserved_enum1),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_enum2",
        offset_of!(lzma_mt, reserved_enum2),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_enum3",
        offset_of!(lzma_mt, reserved_enum3),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_int1",
        offset_of!(lzma_mt, reserved_int1),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_int2",
        offset_of!(lzma_mt, reserved_int2),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_int3",
        offset_of!(lzma_mt, reserved_int3),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_int4",
        offset_of!(lzma_mt, reserved_int4),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "memlimit_threading",
        offset_of!(lzma_mt, memlimit_threading),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "memlimit_stop",
        offset_of!(lzma_mt, memlimit_stop),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_int7",
        offset_of!(lzma_mt, reserved_int7),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_int8",
        offset_of!(lzma_mt, reserved_int8),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_ptr1",
        offset_of!(lzma_mt, reserved_ptr1),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_ptr2",
        offset_of!(lzma_mt, reserved_ptr2),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_ptr3",
        offset_of!(lzma_mt, reserved_ptr3),
    );
    assert_field(
        &fields,
        "lzma_mt",
        "reserved_ptr4",
        offset_of!(lzma_mt, reserved_ptr4),
    );

    assert_field(&fields, "lzma_filter", "id", offset_of!(lzma_filter, id));
    assert_field(
        &fields,
        "lzma_filter",
        "options",
        offset_of!(lzma_filter, options),
    );

    assert_field(
        &fields,
        "lzma_block",
        "version",
        offset_of!(lzma_block, version),
    );
    assert_field(
        &fields,
        "lzma_block",
        "header_size",
        offset_of!(lzma_block, header_size),
    );
    assert_field(
        &fields,
        "lzma_block",
        "check",
        offset_of!(lzma_block, check),
    );
    assert_field(
        &fields,
        "lzma_block",
        "compressed_size",
        offset_of!(lzma_block, compressed_size),
    );
    assert_field(
        &fields,
        "lzma_block",
        "uncompressed_size",
        offset_of!(lzma_block, uncompressed_size),
    );
    assert_field(
        &fields,
        "lzma_block",
        "filters",
        offset_of!(lzma_block, filters),
    );
    assert_field(
        &fields,
        "lzma_block",
        "raw_check",
        offset_of!(lzma_block, raw_check),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_ptr1",
        offset_of!(lzma_block, reserved_ptr1),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_ptr2",
        offset_of!(lzma_block, reserved_ptr2),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_ptr3",
        offset_of!(lzma_block, reserved_ptr3),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_int1",
        offset_of!(lzma_block, reserved_int1),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_int2",
        offset_of!(lzma_block, reserved_int2),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_int3",
        offset_of!(lzma_block, reserved_int3),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_int4",
        offset_of!(lzma_block, reserved_int4),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_int5",
        offset_of!(lzma_block, reserved_int5),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_int6",
        offset_of!(lzma_block, reserved_int6),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_int7",
        offset_of!(lzma_block, reserved_int7),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_int8",
        offset_of!(lzma_block, reserved_int8),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_enum1",
        offset_of!(lzma_block, reserved_enum1),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_enum2",
        offset_of!(lzma_block, reserved_enum2),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_enum3",
        offset_of!(lzma_block, reserved_enum3),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_enum4",
        offset_of!(lzma_block, reserved_enum4),
    );
    assert_field(
        &fields,
        "lzma_block",
        "ignore_check",
        offset_of!(lzma_block, ignore_check),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_bool2",
        offset_of!(lzma_block, reserved_bool2),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_bool3",
        offset_of!(lzma_block, reserved_bool3),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_bool4",
        offset_of!(lzma_block, reserved_bool4),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_bool5",
        offset_of!(lzma_block, reserved_bool5),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_bool6",
        offset_of!(lzma_block, reserved_bool6),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_bool7",
        offset_of!(lzma_block, reserved_bool7),
    );
    assert_field(
        &fields,
        "lzma_block",
        "reserved_bool8",
        offset_of!(lzma_block, reserved_bool8),
    );

    assert_field(
        &fields,
        "lzma_stream_flags",
        "version",
        offset_of!(lzma_stream_flags, version),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "backward_size",
        offset_of!(lzma_stream_flags, backward_size),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "check",
        offset_of!(lzma_stream_flags, check),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_enum1",
        offset_of!(lzma_stream_flags, reserved_enum1),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_enum2",
        offset_of!(lzma_stream_flags, reserved_enum2),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_enum3",
        offset_of!(lzma_stream_flags, reserved_enum3),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_enum4",
        offset_of!(lzma_stream_flags, reserved_enum4),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_bool1",
        offset_of!(lzma_stream_flags, reserved_bool1),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_bool2",
        offset_of!(lzma_stream_flags, reserved_bool2),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_bool3",
        offset_of!(lzma_stream_flags, reserved_bool3),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_bool4",
        offset_of!(lzma_stream_flags, reserved_bool4),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_bool5",
        offset_of!(lzma_stream_flags, reserved_bool5),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_bool6",
        offset_of!(lzma_stream_flags, reserved_bool6),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_bool7",
        offset_of!(lzma_stream_flags, reserved_bool7),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_bool8",
        offset_of!(lzma_stream_flags, reserved_bool8),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_int1",
        offset_of!(lzma_stream_flags, reserved_int1),
    );
    assert_field(
        &fields,
        "lzma_stream_flags",
        "reserved_int2",
        offset_of!(lzma_stream_flags, reserved_int2),
    );

    assert_field(
        &fields,
        "lzma_options_lzma",
        "dict_size",
        offset_of!(lzma_options_lzma, dict_size),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "preset_dict",
        offset_of!(lzma_options_lzma, preset_dict),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "preset_dict_size",
        offset_of!(lzma_options_lzma, preset_dict_size),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "lc",
        offset_of!(lzma_options_lzma, lc),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "lp",
        offset_of!(lzma_options_lzma, lp),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "pb",
        offset_of!(lzma_options_lzma, pb),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "mode",
        offset_of!(lzma_options_lzma, mode),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "nice_len",
        offset_of!(lzma_options_lzma, nice_len),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "mf",
        offset_of!(lzma_options_lzma, mf),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "depth",
        offset_of!(lzma_options_lzma, depth),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "ext_flags",
        offset_of!(lzma_options_lzma, ext_flags),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "ext_size_low",
        offset_of!(lzma_options_lzma, ext_size_low),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "ext_size_high",
        offset_of!(lzma_options_lzma, ext_size_high),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_int4",
        offset_of!(lzma_options_lzma, reserved_int4),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_int5",
        offset_of!(lzma_options_lzma, reserved_int5),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_int6",
        offset_of!(lzma_options_lzma, reserved_int6),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_int7",
        offset_of!(lzma_options_lzma, reserved_int7),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_int8",
        offset_of!(lzma_options_lzma, reserved_int8),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_enum1",
        offset_of!(lzma_options_lzma, reserved_enum1),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_enum2",
        offset_of!(lzma_options_lzma, reserved_enum2),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_enum3",
        offset_of!(lzma_options_lzma, reserved_enum3),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_enum4",
        offset_of!(lzma_options_lzma, reserved_enum4),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_ptr1",
        offset_of!(lzma_options_lzma, reserved_ptr1),
    );
    assert_field(
        &fields,
        "lzma_options_lzma",
        "reserved_ptr2",
        offset_of!(lzma_options_lzma, reserved_ptr2),
    );

    assert_field(
        &fields,
        "lzma_options_delta",
        "type",
        offset_of!(lzma_options_delta, r#type),
    );
    assert_field(
        &fields,
        "lzma_options_delta",
        "dist",
        offset_of!(lzma_options_delta, dist),
    );
    assert_field(
        &fields,
        "lzma_options_delta",
        "reserved_int1",
        offset_of!(lzma_options_delta, reserved_int1),
    );
    assert_field(
        &fields,
        "lzma_options_delta",
        "reserved_int2",
        offset_of!(lzma_options_delta, reserved_int2),
    );
    assert_field(
        &fields,
        "lzma_options_delta",
        "reserved_int3",
        offset_of!(lzma_options_delta, reserved_int3),
    );
    assert_field(
        &fields,
        "lzma_options_delta",
        "reserved_int4",
        offset_of!(lzma_options_delta, reserved_int4),
    );
    assert_field(
        &fields,
        "lzma_options_delta",
        "reserved_ptr1",
        offset_of!(lzma_options_delta, reserved_ptr1),
    );
    assert_field(
        &fields,
        "lzma_options_delta",
        "reserved_ptr2",
        offset_of!(lzma_options_delta, reserved_ptr2),
    );

    assert_field(
        &fields,
        "lzma_options_bcj",
        "start_offset",
        offset_of!(lzma_options_bcj, start_offset),
    );

    let stream_base = offset_of!(lzma_index_iter, stream);
    let block_base = offset_of!(lzma_index_iter, block);
    assert_field(&fields, "lzma_index_iter", "stream", stream_base);
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.flags",
        stream_base + offset_of!(lzma_index_iter_stream, flags),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.reserved_ptr1",
        stream_base + offset_of!(lzma_index_iter_stream, reserved_ptr1),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.reserved_ptr2",
        stream_base + offset_of!(lzma_index_iter_stream, reserved_ptr2),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.reserved_ptr3",
        stream_base + offset_of!(lzma_index_iter_stream, reserved_ptr3),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.number",
        stream_base + offset_of!(lzma_index_iter_stream, number),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.block_count",
        stream_base + offset_of!(lzma_index_iter_stream, block_count),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.compressed_offset",
        stream_base + offset_of!(lzma_index_iter_stream, compressed_offset),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.uncompressed_offset",
        stream_base + offset_of!(lzma_index_iter_stream, uncompressed_offset),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.compressed_size",
        stream_base + offset_of!(lzma_index_iter_stream, compressed_size),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.uncompressed_size",
        stream_base + offset_of!(lzma_index_iter_stream, uncompressed_size),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.padding",
        stream_base + offset_of!(lzma_index_iter_stream, padding),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.reserved_vli1",
        stream_base + offset_of!(lzma_index_iter_stream, reserved_vli1),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.reserved_vli2",
        stream_base + offset_of!(lzma_index_iter_stream, reserved_vli2),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.reserved_vli3",
        stream_base + offset_of!(lzma_index_iter_stream, reserved_vli3),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "stream.reserved_vli4",
        stream_base + offset_of!(lzma_index_iter_stream, reserved_vli4),
    );

    assert_field(&fields, "lzma_index_iter", "block", block_base);
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.number_in_file",
        block_base + offset_of!(lzma_index_iter_block, number_in_file),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.compressed_file_offset",
        block_base + offset_of!(lzma_index_iter_block, compressed_file_offset),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.uncompressed_file_offset",
        block_base + offset_of!(lzma_index_iter_block, uncompressed_file_offset),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.number_in_stream",
        block_base + offset_of!(lzma_index_iter_block, number_in_stream),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.compressed_stream_offset",
        block_base + offset_of!(lzma_index_iter_block, compressed_stream_offset),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.uncompressed_stream_offset",
        block_base + offset_of!(lzma_index_iter_block, uncompressed_stream_offset),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.uncompressed_size",
        block_base + offset_of!(lzma_index_iter_block, uncompressed_size),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.unpadded_size",
        block_base + offset_of!(lzma_index_iter_block, unpadded_size),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.total_size",
        block_base + offset_of!(lzma_index_iter_block, total_size),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.reserved_vli1",
        block_base + offset_of!(lzma_index_iter_block, reserved_vli1),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.reserved_vli2",
        block_base + offset_of!(lzma_index_iter_block, reserved_vli2),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.reserved_vli3",
        block_base + offset_of!(lzma_index_iter_block, reserved_vli3),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.reserved_vli4",
        block_base + offset_of!(lzma_index_iter_block, reserved_vli4),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.reserved_ptr1",
        block_base + offset_of!(lzma_index_iter_block, reserved_ptr1),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.reserved_ptr2",
        block_base + offset_of!(lzma_index_iter_block, reserved_ptr2),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.reserved_ptr3",
        block_base + offset_of!(lzma_index_iter_block, reserved_ptr3),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "block.reserved_ptr4",
        block_base + offset_of!(lzma_index_iter_block, reserved_ptr4),
    );
    assert_field(
        &fields,
        "lzma_index_iter",
        "internal",
        offset_of!(lzma_index_iter, internal),
    );
}
