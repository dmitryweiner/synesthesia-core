// Semi-Lagrangian advection: sample the state from where it "came from"
// this step per the velocity field. Runs once per rendered frame after the
// reaction substeps — "how fast it reacts" and "how fast it flows" are
// independent knobs. uAdvectAmount = 0 degenerates to an identity copy.
in vec2 vUv;
out vec4 fragColor;

uniform sampler2D uState;
uniform sampler2D uVelocity;
uniform float uAdvectAmount;

void main() {
  vec2 vel = texture(uVelocity, vUv).rg;
  vec2 srcUv = vUv - vel * uAdvectAmount;
  fragColor = texture(uState, srcUv);
}
