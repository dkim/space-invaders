in vec2 texture_coord;

uniform sampler2D sampler;

out vec4 color;

void main() {
  color = vec4(texture(sampler, texture_coord).rrr, 1.);
}
