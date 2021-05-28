import { mat4, ReadonlyVec3, vec3, vec4 } from 'gl-matrix'
import { initShaderProgram } from './glutil'
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
  attribute vec4 aVertexPosition;
   attribute vec4 aVertexColor;
   uniform mat4 uModelViewMatrix;
   uniform mat4 uProjectionMatrix;
   varying lowp vec4 vColor;
   void main(void) {
     gl_Position = uProjectionMatrix * uModelViewMatrix * aVertexPosition;
     vColor = aVertexColor;
   }
  `

  // Fragment shader program

  let fsSource = `
   precision mediump float;
    void main(void) {
      gl_FragColor = vec4(1.0, 0.0, 0.0, 1.0);
    }
  `

  let shaderProgram = initShaderProgram(gl, vsSource, fsSource)

  if (!shaderProgram) {
    throw 'todo create shader'
  }

  gl.useProgram(shaderProgram)

  let firePosUni = gl.getUniformLocation(shaderProgram, 'uFirePos')!
  let perspectiveUni = gl.getUniformLocation(shaderProgram, 'uPMatrix')!
  let viewUni = gl.getUniformLocation(shaderProgram, 'uViewMatrix')!
  let fireAtlasUni = gl.getUniformLocation(shaderProgram, 'uFireAtlas')!
  let sdfUni = gl.getUniformLocation(shaderProgram, 'uSdf')!

  let projectionMatrix = gl.getUniformLocation(
    shaderProgram,
    'uProjectionMatrix',
  )!
  let modelViewMatrix = gl.getUniformLocation(
    shaderProgram,
    'uModelViewMatrix',
  )!

  let texCoordAttrib = gl.getAttribLocation(shaderProgram, 'aTextureCoords')
  let triCornerAttrib = gl.getAttribLocation(shaderProgram, 'aTriCorner')
  let centerOffsetAttrib = gl.getAttribLocation(shaderProgram, 'aCenterOffset')
  let vertexPositionAttrib = gl.getAttribLocation(
    shaderProgram,
    'aVertexPosition',
  )

  let posBuffer = initBuffers(gl)

  {
    let buffers = posBuffer
    let numComponents = 2 // pull out 2 values per iteration
    let type = gl.FLOAT // the data in the buffer is 32bit floats
    let normalize = false // don't normalize
    let stride = 0 // how many bytes to get from one set of values to the next
    // 0 = use type and numComponents above
    let offset = 0 // how many bytes inside the buffer to start from
    gl.bindBuffer(gl.ARRAY_BUFFER, buffers)
    gl.vertexAttribPointer(
      vertexPositionAttrib,
      numComponents,
      type,
      normalize,
      stride,
      offset,
    )
    gl.enableVertexAttribArray(vertexPositionAttrib)
  }

  let sdf = wasm.march()
  let sdfTexture = createDataTexture(gl, sdf)

  let state: State = {
    wasm,
    render: {
      sdfUni,
      sdfTexture,
      viewUni,

      projectionMatrix,
      modelViewMatrix,
      posBuffer,

      xRotation: 0,
      yRotation: 0,
      imageIsLoaded: false,
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

  // We set OpenGL's blend function so that we don't see the black background
  // on our particle squares. Essentially, if there is anything behind the particle
  // we show whatever is behind it plus the color of the particle.
  //
  // If the color of the particle is black then black is (0, 0, 0) so we only show
  // whatever is behind it.
  // So this works because our texture has a black background.
  // There are many different blend functions that you can use, this one works for our
  // purposes.
  // gl.enable(gl.DEPTH_TEST)
  // gl.depthFunc(gl.LESS)
  gl.enable(gl.BLEND)
  gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)

  // Push our fire texture atlas to the GPU
  gl.activeTexture(gl.TEXTURE0)
  gl.bindTexture(gl.TEXTURE_2D, fireTexture)
  gl.uniform1i(fireAtlasUni, 0)

  // sdf
  gl.activeTexture(gl.TEXTURE1)
  gl.bindTexture(gl.TEXTURE_2D, sdfTexture)
  gl.uniform1i(sdfUni, 1)

  // Send our perspective matrix to the GPU
  gl.uniformMatrix4fv(
    perspectiveUni,
    false,
    mat4.perspective([] as any as mat4, Math.PI / 3, 1.333, 0.01, 100),
  )

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

function initBuffers(gl: WebGLRenderingContext): WebGLBuffer {
  // Create a buffer for the square's positions.

  let positionBuffer = gl.createBuffer()
  if (!positionBuffer) {
    throw 'todo buffer'
  }

  // Select the positionBuffer as the one to apply buffer
  // operations to from here out.

  gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer)

  // Now create an array of positions for the square.

  let positions = [0, 1.0, 1.0, 1.0, 0, 0, 1.0, 0]

  // Now pass the list of positions into WebGL to build the
  // shape. We do this by creating a Float32Array from the
  // JavaScript array, then use it to fill the current buffer.

  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(positions), gl.STATIC_DRAW)

  return positionBuffer
}

function createCameraUniform(xRotation: number, yRotation: number) {
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
  mat4.lookAt(camera, cameraPos, [0, 0, 0], [0, 1, 0])

  return camera
}

interface State {
  wasm: WasmModule
  render: RenderState
}

interface GameState {}

interface RenderState {
  sdfTexture: WebGLBuffer
  shader: WebGLProgram
  posBuffer: WebGLBuffer
  viewUni: WebGLUniformLocation
  sdfUni: WebGLUniformLocation
  projectionMatrix: WebGLUniformLocation
  modelViewMatrix: WebGLUniformLocation
  imageIsLoaded: boolean
  xRotation: number
  yRotation: number
  lastMouseX: number
  lastMouseY: number
}

function draw(state: State, gl: WebGLRenderingContext) {
  let render = state.render

  // {
  //   let xSpeed = 0
  //   let ySpeed = 0.4
  //   state.render.xRotation += xSpeed / 50
  //   state.render.yRotation -= ySpeed / 50

  //   state.render.xRotation = Math.min(state.render.xRotation, Math.PI / 2.5)
  //   state.render.xRotation = Math.max(state.render.xRotation, -Math.PI / 2.5)
  // }

  // Once the image is loaded we'll start drawing our particle effect
  if (render.imageIsLoaded) {
    // Clear our color buffer and depth buffer so that
    // nothing is left over in our drawing buffer now that we're
    // completely redrawing the entire canvas
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)

    gl.useProgram(render.shader)

    // Pass our world view matrix into our vertex shader
    // gl.uniformMatrix4fv(
    //   render.viewUni,
    //   false,
    //   createCameraUniform(render.xRotation, render.yRotation),
    // )

    let fieldOfView = (45 * Math.PI) / 180 // in radians
    // let aspect = gl.canvas.clientWidth / gl.canvas.clientHeight
    let aspect = 1.3
    let zNear = 0.1
    let zFar = 100.0
    let projectionMatrix = mat4.create()

    // note: glmatrix.js always has the first argument
    // as the destination to receive the result.
    mat4.perspective(projectionMatrix, fieldOfView, aspect, zNear, zFar)

    let modelViewMatrix = mat4.create()

    mat4.translate(modelViewMatrix, modelViewMatrix, [-0.5, -0.5, -0.1])

    gl.uniformMatrix4fv(render.projectionMatrix, false, projectionMatrix)
    gl.uniformMatrix4fv(render.modelViewMatrix, false, modelViewMatrix)

    let offset = 0
    let vertexCount = 4
    gl.drawArrays(gl.TRIANGLE_STRIP, offset, vertexCount)
  }

  // On the next animation frame we re-draw our particle effect
  window.requestAnimationFrame(() => draw(state, gl))
}

function createDataTexture(
  gl: WebGLRenderingContext,
  data: Float32Array,
): WebGLTexture {
  var ext = gl.getExtension('OES_texture_float')

  var texture = gl.createTexture()
  if (!texture) {
    throw 'todo create data'
  }
  gl.bindTexture(gl.TEXTURE_2D, texture)
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.FLOAT, data)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
  return texture
}

function aperture<T>(n: number, arr: T[]): T[][] {
  return n > arr.length
    ? []
    : arr.slice(n - 1).map((v, i) => arr.slice(i, i + n))
}
