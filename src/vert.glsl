attribute vec4 aVertexPosition;
attribute vec2 aUv;

varying lowp vec4 vColor;
void main(void) {
  gl_Position = aVertexPosition - 0.5;
}