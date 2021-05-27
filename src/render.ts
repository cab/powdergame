import { mat4, ReadonlyVec3, vec3, vec4 } from 'gl-matrix'
import { WasmModule } from './index'

interface Buffers {
  position: WebGLBuffer
  color: WebGLBuffer
}

export function render(wasm: WasmModule, canvas: HTMLCanvasElement) {
  let gl = canvas.getContext('webgl')
  if (!gl) {
    alert(
      'Unable to initialize WebGL. Your browser or machine may not support it.',
    )
    return
  }

  // Vertex shader program

  let vsSource = `

uniform vec3 uFirePos;

attribute vec2 aTextureCoords;

attribute vec2 aTriCorner;

attribute vec3 aCenterOffset;

uniform mat4 uPMatrix;
uniform mat4 uViewMatrix;

varying vec2 vTextureCoords;

void main (void) {

  vec4 position = vec4(
    uFirePos + aCenterOffset,
    1.0
  );

  float size = 0.05;

  vec3 cameraRight = vec3(
    uViewMatrix[0].x, uViewMatrix[1].x, uViewMatrix[2].x
  );
  vec3 cameraUp = vec3(
    uViewMatrix[0].y, uViewMatrix[1].y, uViewMatrix[2].y
  );

  position.xyz += (cameraRight * aTriCorner.x * size) +
    (cameraUp * aTriCorner.y * size);
  
  gl_Position = uPMatrix * uViewMatrix * position;

  vTextureCoords = aTextureCoords;
}
  `

  // Fragment shader program

  let fsSource = `
   precision mediump float;

varying vec2 vTextureCoords;

uniform sampler2D fireAtlas;

void main (void) {
  vec4 texColor = texture2D(
    fireAtlas, 
    vec2(
      (vTextureCoords.x / 2.0),
      (vTextureCoords.y / 2.0)
  ));
  if(texColor.a < 0.1) {
    discard;
  }
  gl_FragColor = texColor;

}
  `

  let shaderProgram = initShaderProgram(gl, vsSource, fsSource)

  if (!shaderProgram) {
    throw 'todo'
  }

  gl.useProgram(shaderProgram)

  let firePosUni = gl.getUniformLocation(shaderProgram, 'uFirePos')!
  let perspectiveUni = gl.getUniformLocation(shaderProgram, 'uPMatrix')!
  let viewUni = gl.getUniformLocation(shaderProgram, 'uViewMatrix')!
  let fireAtlasUni = gl.getUniformLocation(shaderProgram, 'uFireAtlas')!

  let lifetimeAttrib = gl.getAttribLocation(shaderProgram, 'aLifetime')
  let texCoordAttrib = gl.getAttribLocation(shaderProgram, 'aTextureCoords')
  let triCornerAttrib = gl.getAttribLocation(shaderProgram, 'aTriCorner')
  let centerOffsetAttrib = gl.getAttribLocation(shaderProgram, 'aCenterOffset')
  let velocityAttrib = gl.getAttribLocation(shaderProgram, 'aVelocity')
  gl.enableVertexAttribArray(lifetimeAttrib)
  gl.enableVertexAttribArray(texCoordAttrib)
  gl.enableVertexAttribArray(triCornerAttrib)
  gl.enableVertexAttribArray(centerOffsetAttrib)
  gl.enableVertexAttribArray(velocityAttrib)
  let state: State = {
    wasm,
    render: {
      firePosUni,
      viewUni,
      lifetimeAttrib,
      texCoordAttrib,
      triCornerAttrib,
      centerOffsetAttrib,
      velocityAttrib,
      xRotation: 0,
      yRotation: 0,
      imageIsLoaded: false,
      numParticles: 200,
      redFirePos: [0.0, 0.0, 0.0],
      shader: shaderProgram,
      lastMouseX: 0,
      lastMouseY: 0,
    },
  }

  {
    var fireTexture = gl.createTexture()
    var fireAtlas = new window.Image()
    fireAtlas.onload = function () {
      if (!gl) {
        alert('failed to load texture in context')
        return
      }
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true)
      gl.bindTexture(gl.TEXTURE_2D, fireTexture)
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        fireAtlas,
      )
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
      // todo event
      state.render.imageIsLoaded = true
    }
    fireAtlas.src = '/assets/rock.png'
  }

  let triCornersCycle = [
    // Bottom left corner of the square
    -1.0, -1.0,
    // Bottom right corner of the square
    1.0, -1.0,
    // Top right corner of the square
    1.0, 1.0,
    // Top left corner of the square
    -1.0, 1.0,
  ]
  let texCoordsCycle = [
    // Bottom left corner of the texture
    0, 0,
    // Bottom right corner of the texture
    1, 0,
    // Top right corner of the texture
    1, 1,
    // Top left corner of the texture
    0, 1,
  ]

  {
    let { triCorners, centerOffsets, vertexIndices, texCoords } =
      createVertexIndices(state.render.numParticles)
    function createBuffer(
      bufferType: 'ARRAY_BUFFER' | 'ELEMENT_ARRAY_BUFFER',
      DataType: any,
      data: any,
    ) {
      if (!gl) {
        throw 'todo'
      }
      var buffer = gl.createBuffer()
      gl.bindBuffer(gl[bufferType], buffer)
      gl.bufferData(gl[bufferType], new DataType(data), gl.STATIC_DRAW)
      return buffer
    }

    createBuffer('ARRAY_BUFFER', Float32Array, texCoords)
    gl.vertexAttribPointer(texCoordAttrib, 2, gl.FLOAT, false, 0, 0)

    createBuffer('ARRAY_BUFFER', Float32Array, triCorners)
    gl.vertexAttribPointer(triCornerAttrib, 2, gl.FLOAT, false, 0, 0)

    createBuffer('ARRAY_BUFFER', Float32Array, centerOffsets)
    gl.vertexAttribPointer(centerOffsetAttrib, 3, gl.FLOAT, false, 0, 0)

    createBuffer('ELEMENT_ARRAY_BUFFER', Uint16Array, vertexIndices)

    // We set OpenGL's blend function so that we don't see the black background
    // on our particle squares. Essentially, if there is anything behind the particle
    // we show whatever is behind it plus the color of the particle.
    //
    // If the color of the particle is black then black is (0, 0, 0) so we only show
    // whatever is behind it.
    // So this works because our texture has a black background.
    // There are many different blend functions that you can use, this one works for our
    // purposes.
    gl.enable(gl.DEPTH_TEST)
    gl.depthFunc(gl.LESS)
    gl.enable(gl.BLEND)
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)

    // Push our fire texture atlas to the GPU
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, fireTexture)
    gl.uniform1i(fireAtlasUni, 0)

    // Send our perspective matrix to the GPU
    gl.uniformMatrix4fv(
      perspectiveUni,
      false,
      mat4.perspective([] as any as mat4, Math.PI / 3, 1, 0.01, 1000),
    )
  }

  canvas.addEventListener('mousemove', (e) => {
    // if (isDragging) {
    state.render.xRotation += (e.pageY - state.render.lastMouseY) / 50
    state.render.yRotation -= (e.pageX - state.render.lastMouseX) / 50

    state.render.xRotation = Math.min(state.render.xRotation, Math.PI / 2.5)
    state.render.xRotation = Math.max(state.render.xRotation, -Math.PI / 2.5)

    state.render.lastMouseX = e.pageX
    state.render.lastMouseY = e.pageY
    // }
  })

  draw(state, gl)
}

//
// Initialize a shader program, so WebGL knows how to draw our data
//
function initShaderProgram(
  gl: WebGLRenderingContext,
  vsSource: string,
  fsSource: string,
) {
  let vertexShader = loadShader(gl, gl.VERTEX_SHADER, vsSource)
  let fragmentShader = loadShader(gl, gl.FRAGMENT_SHADER, fsSource)

  let shaderProgram = gl.createProgram()
  if (!shaderProgram || !vertexShader || !fragmentShader) {
    throw 'todo'
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

//
// creates a shader of the given type, uploads the source and
// compiles it.
//
function loadShader(gl: WebGLRenderingContext, type: GLenum, source: string) {
  let shader = gl.createShader(type)

  if (!shader) {
    throw 'todo'
  }

  // Send the source to the shader object

  gl.shaderSource(shader, source)

  // Compile the shader program

  gl.compileShader(shader)

  // See if it compiled successfully

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    alert(
      'An error occurred compiling the shaders: ' + gl.getShaderInfoLog(shader),
    )
    gl.deleteShader(shader)
    return null
  }

  return shader
}

function createVertexIndices(numParticles: number) {
  let triCorners = []
  let texCoords = []
  let vertexIndices: number[][] = []
  let centerOffsets = []
  let velocities = []

  let triCornersCycle = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0]
  let texCoordsCycle = [0, 0, 1, 0, 1, 1, 0, 1]

  for (let i = 0; i < numParticles; i++) {
    let diameterAroundCenter = 2.0
    let halfDiameterAroundCenter = diameterAroundCenter / 2

    let xStartOffset =
      diameterAroundCenter * Math.random() - halfDiameterAroundCenter
    xStartOffset /= 3

    let yStartOffset =
      diameterAroundCenter * Math.random() - halfDiameterAroundCenter
    yStartOffset /= 10

    let zStartOffset =
      diameterAroundCenter * Math.random() - halfDiameterAroundCenter
    zStartOffset /= 3

    for (let j = 0; j < 4; j++) {
      triCorners.push(triCornersCycle[j * 2])
      triCorners.push(triCornersCycle[j * 2 + 1])

      texCoords.push(texCoordsCycle[j * 2])
      texCoords.push(texCoordsCycle[j * 2 + 1])

      centerOffsets.push(xStartOffset)
      centerOffsets.push(yStartOffset + Math.abs(xStartOffset / 2.0))
      centerOffsets.push(zStartOffset)
    }

    vertexIndices = vertexIndices.concat(
      [0, 1, 2, 0, 2, 3].map(function (num) {
        return num + 4 * i
      }),
    )
  }

  return {
    texCoords,
    vertexIndices,
    centerOffsets,
    triCorners,
  }
}

function createCameraUniform(
  xRotation: number,
  yRotation: number,
  redFirePos: ReadonlyVec3,
) {
  var camera = mat4.create()

  // Start our camera off at a height of 0.25 and 1 unit
  // away from the origin
  mat4.translate(camera, camera, [0, 0.25, 1])

  // Rotate our camera around the y and x axis of the world
  // as the viewer clicks or drags their finger
  var xAxisRotation = mat4.create()
  var yAxisRotation = mat4.create()
  mat4.rotateX(xAxisRotation, xAxisRotation, -xRotation)
  mat4.rotateY(yAxisRotation, yAxisRotation, yRotation)
  mat4.multiply(camera, xAxisRotation, camera)
  mat4.multiply(camera, yAxisRotation, camera)

  // Make our camera look at the first red fire
  var cameraPos: ReadonlyVec3 = [camera[12], camera[13], camera[14]]
  mat4.lookAt(camera, cameraPos, redFirePos, [0, 1, 0])

  return camera
}

interface State {
  wasm: WasmModule
  render: RenderState
}

interface GameState {}

interface RenderState {
  shader: WebGLProgram
  viewUni: WebGLUniformLocation
  firePosUni: WebGLUniformLocation
  lifetimeAttrib: number
  texCoordAttrib: number
  triCornerAttrib: number
  centerOffsetAttrib: number
  velocityAttrib: number
  redFirePos: vec3
  xRotation: number
  yRotation: number
  imageIsLoaded: boolean
  numParticles: number
  lastMouseX: number
  lastMouseY: number
}

function draw(state: State, gl: WebGLRenderingContext) {
  let sdf = state.wasm.calculate_sdf()
  console.log(sdf.length)

  let render = state.render
  // Once the image is loaded we'll start drawing our particle effect
  if (render.imageIsLoaded) {
    // Clear our color buffer and depth buffer so that
    // nothing is left over in our drawing buffer now that we're
    // completely redrawing the entire canvas
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)

    gl.useProgram(render.shader)

    // Pass our world view matrix into our vertex shader
    gl.uniformMatrix4fv(
      render.viewUni,
      false,
      createCameraUniform(
        render.xRotation,
        render.yRotation,
        render.redFirePos,
      ),
    )

    // We pass information specific to our first flame into our vertex shader
    // and then draw our first flame.
    gl.uniform3fv(render.firePosUni, render.redFirePos)
    // What does numParticles * 6 mean?
    //  For each particle there are two triangles drawn (to form the square)
    //  The first triangle has 3 vertices and the second triangle has 3 vertices
    //  making for a total of 6 vertices per particle.
    gl.drawElements(gl.TRIANGLES, render.numParticles * 6, gl.UNSIGNED_SHORT, 0)

    // We pass information specific to our second flame into our vertex shader
    // and then draw our second flame.
    // gl.uniform3fv(render.firePosUni, render.purpFirePos)
    // gl.uniform4fv(render.colorUni, purpFireColor)
    // gl.drawElements(gl.TRIANGLES, numParticles * 6, gl.UNSIGNED_SHORT, 0)
  }

  // On the next animation frame we re-draw our particle effect
  window.requestAnimationFrame(() => draw(state, gl))
}
