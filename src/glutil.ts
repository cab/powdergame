// initialize a shader program
export function initShaderProgram(
  gl: WebGLRenderingContext,
  vsSource: string,
  fsSource: string,
) {
  let vertexShader = loadShader(gl, gl.VERTEX_SHADER, vsSource)
  let fragmentShader = loadShader(gl, gl.FRAGMENT_SHADER, fsSource)

  let shaderProgram = gl.createProgram()
  if (!shaderProgram || !vertexShader || !fragmentShader) {
    throw 'failed to create shader'
  }
  gl.attachShader(shaderProgram, vertexShader)
  gl.attachShader(shaderProgram, fragmentShader)
  gl.linkProgram(shaderProgram)

  // If creating the shader program failed, alert

  if (!gl.getProgramParameter(shaderProgram, gl.LINK_STATUS)) {
    alert(
      'Unable to initialize the shader program: ' +
        gl.getProgramInfoLog(shaderProgram),
    )
    return null
  }

  return shaderProgram
}

// creates a shader of the given type, uploads the source and
// compiles it.
function loadShader(gl: WebGLRenderingContext, type: GLenum, source: string) {
  let shader = gl.createShader(type)

  if (!shader) {
    throw 'failed to load shader'
  }

  gl.shaderSource(shader, source)

  gl.compileShader(shader)

  // see if it compiled successfully
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    alert(
      'An error occurred compiling the shaders: ' + gl.getShaderInfoLog(shader),
    )
    gl.deleteShader(shader)
    return null
  }

  return shader
}
