//! Conversión de datos puros (geometria.rs) a recursos de Godot.

use godot::classes::{mesh, ArrayMesh};
use godot::prelude::*;

use crate::geometria::MallaDatos;

pub fn a_array_mesh(m: &MallaDatos) -> Gd<ArrayMesh> {
    let vertices: PackedVector3Array = m
        .vertices
        .iter()
        .map(|v| Vector3::new(v[0], v[1], v[2]))
        .collect();
    let normales: PackedVector3Array = m
        .normales
        .iter()
        .map(|v| Vector3::new(v[0], v[1], v[2]))
        .collect();
    let colores: PackedColorArray = m
        .colores
        .iter()
        .map(|c| Color::from_rgba(c[0], c[1], c[2], c[3]))
        .collect();
    let uvs: PackedVector2Array = m.uvs.iter().map(|u| Vector2::new(u[0], u[1])).collect();
    let indices: PackedInt32Array = m.indices.iter().copied().collect();

    let mut arrays = VarArray::new();
    arrays.resize(mesh::ArrayType::MAX.ord() as usize, &Variant::nil());
    arrays.set(mesh::ArrayType::VERTEX.ord() as usize, &vertices.to_variant());
    arrays.set(mesh::ArrayType::NORMAL.ord() as usize, &normales.to_variant());
    arrays.set(mesh::ArrayType::COLOR.ord() as usize, &colores.to_variant());
    arrays.set(mesh::ArrayType::TEX_UV.ord() as usize, &uvs.to_variant());
    arrays.set(mesh::ArrayType::INDEX.ord() as usize, &indices.to_variant());

    let mut malla = ArrayMesh::new_gd();
    malla.add_surface_from_arrays(mesh::PrimitiveType::TRIANGLES, &arrays);
    malla
}

/// Argumentos de usuario de la línea de comandos (lo que va después de `--`).
/// Formato: `--clave=valor` o `--bandera`.
pub fn argumento_usuario(clave: &str) -> Option<String> {
    let args = godot::classes::Os::singleton().get_cmdline_user_args();
    let prefijo = format!("--{clave}");
    for a in args.as_slice() {
        let s = a.to_string();
        if s == prefijo {
            return Some(String::new());
        }
        if let Some(resto) = s.strip_prefix(&format!("{prefijo}=")) {
            return Some(resto.to_string());
        }
    }
    None
}
