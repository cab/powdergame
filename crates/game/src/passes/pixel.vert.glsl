attribute vec4 a_vertex_position;
uniform mat4 u_projection;
void main() {
  gl_Position = u_projection * a_vertex_position;
}