shader_type spatial;
render_mode wireframe;

uniform sampler2D terrain_texture : source_color, filter_linear_mipmap_anisotropic;

void vertex() {
	vec4 tex = texture(terrain_texture, UV);
	VERTEX.y = tex.r / 2.0;
}

void fragment() {
	vec2 tc = UV;
	vec4 tex = texture(terrain_texture, tc);
	ALBEDO = vec3(tex.r * tex.r);
}
