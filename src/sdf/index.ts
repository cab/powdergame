interface ConstantSdfNode {
  kind: 'constant'
}

type SDFNode = ConstantSdfNode

// converts an SDF node into a shader source
function compile(node: Node): string {
  return 'TODO'
}
