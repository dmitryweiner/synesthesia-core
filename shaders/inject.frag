// Onset seeding (PLAN.md decision 8): drops fresh v ("ink") into a disc at
// uCenter, the way seed.frag's spots start a pattern — the reaction then
// grows a new bloom there. uAmount (0..1) scales how much of the disc is
// converted; distances are aspect-corrected so the disc stays round.
in vec2 vUv;
out vec4 fragColor;

uniform sampler2D uState;
uniform vec2 uCenter;
uniform float uRadius;
uniform float uAmount;
uniform float uAspect;

void main() {
  vec4 s = texture(uState, vUv);
  vec2 d = (vUv - uCenter) * vec2(uAspect, 1.0);
  float blob = smoothstep(uRadius, uRadius * 0.3, length(d)) * uAmount;
  float v = max(s.g, blob * 0.5);
  float u = mix(s.r, 0.5, blob);
  fragColor = vec4(u, v, 0.0, 1.0);
}
