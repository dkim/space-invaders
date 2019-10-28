out vec2 texture_coord;

const vec2[4] POSITION = vec2[](
  vec2(-1., -1.), vec2(1., -1.), vec2(1., 1.), vec2(-1., 1.)
);

const vec2[4] TEXTURE_COORD = vec2[](
  vec2(0., 0.), vec2(0., 1.), vec2(1., 1.), vec2(1., 0.)
);

void main() {
  gl_Position = vec4(POSITION[gl_VertexID], 0., 1.);
  texture_coord = TEXTURE_COORD[gl_VertexID];
}
