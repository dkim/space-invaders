in vec2 texture_coord;

uniform sampler2D sampler;

out vec4 color;

void main() {
  color = vec4(texture(sampler, texture_coord).rrr, 1.);
  if (texture_coord.x <= 16. / 256.) {
    if (16. / 224. < texture_coord.y
        && texture_coord.y <= (16. + 118.) / 224.) {
      color = vec4(0., color.g, 0., 1.);
    }
  } else if (texture_coord.x <= (16. + 56.) / 256.) {
    color = vec4(0., color.g, 0., 1.);
  } else if ((16. + 56. + 120.) / 256. < texture_coord.x
             && texture_coord.x <= (16. + 56. + 120. + 32.) / 256.) {
    color = vec4(color.r, 0., 0., 1.);
  }
}
