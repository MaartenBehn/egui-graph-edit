use super::*;

impl<NodeData, DataType, ValueType> Graph<NodeData, DataType, ValueType>
where
    DataType: PartialEq,
{
    pub fn new() -> Self {
        Self {
            nodes: SlotMap::default(),
            inputs: SlotMap::default(),
            outputs: SlotMap::default(),
            connections: SecondaryMap::default(),
        }
    }

    pub fn add_node(
        &mut self,
        label: String,
        user_data: NodeData,
        f: impl FnOnce(&mut Graph<NodeData, DataType, ValueType>, NodeId),
    ) -> NodeId {
        let node_id = self.nodes.insert_with_key(|node_id| {
            Node {
                id: node_id,
                label,
                // These get filled in later by the user function
                inputs: Vec::default(),
                outputs: Vec::default(),
                user_data,
            }
        });

        f(self, node_id);

        node_id
    }

    pub fn add_input_param(
        &mut self,
        node_id: NodeId,
        name: String,
        typ: DataType,
        value: ValueType,
        kind: InputParamKind,
        shown_inline: bool,
    ) -> InputId {
        let input_id = self.inputs.insert_with_key(|input_id| InputParam {
            id: input_id,
            typ,
            value,
            kind,
            node: node_id,
            shown_inline,
        });
        self.nodes[node_id].inputs.push((name, input_id));
        input_id
    }

    pub fn update_input_param(
        &mut self,
        input_id: InputId,
        name: Option<String>,
        typ: Option<DataType>,
        value: Option<ValueType>,
        kind: Option<InputParamKind>,
        shown_inline: Option<bool>,
    ) {
        if let Some(input_param) = self.inputs.get_mut(input_id) {
            if let Some(new_typ) = typ {
                input_param.typ = new_typ;
            }
            if let Some(new_value) = value {
                input_param.value = new_value;
            }
            if let Some(new_kind) = kind {
                input_param.kind = new_kind;
            }
            if let Some(new_shown_inline) = shown_inline {
                input_param.shown_inline = new_shown_inline;
            }

            if let Some(new_name) = name {
                for (curr_name, id) in &mut self.nodes[input_param.node].inputs {
                    if *id == input_id {
                        *curr_name = new_name;
                        break;
                    }
                }
            }
        }

        self.ensure_connection_types(AnyParameterId::Input(input_id));
    }

    pub fn remove_input_param(&mut self, param: InputId) {
        let node = self[param].node;
        self[node].inputs.retain(|(_, id)| *id != param);
        self.inputs.remove(param);
        self.connections.retain(|i, _| i != param);
    }

    pub fn add_output_param(&mut self, node_id: NodeId, name: String, typ: DataType) -> OutputId {
        let output_id = self.outputs.insert_with_key(|output_id| OutputParam {
            id: output_id,
            node: node_id,
            typ,
        });
        self.nodes[node_id].outputs.push((name, output_id));
        output_id
    }

    pub fn update_output_param(
        &mut self,
        output_id: OutputId,
        name: Option<String>,
        typ: Option<DataType>,
    ) {
        if let Some(output_param) = self.outputs.get_mut(output_id) {
            if let Some(new_typ) = typ {
                output_param.typ = new_typ;
            }

            if let Some(new_name) = name {
                for (curr_name, id) in &mut self.nodes[output_param.node].outputs {
                    if *id == output_id {
                        *curr_name = new_name;
                        break;
                    }
                }
            }
        }

        self.ensure_connection_types(AnyParameterId::Output(output_id));
    }

    pub fn remove_output_param(&mut self, param: OutputId) {
        let node = self[param].node;
        self[node].outputs.retain(|(_, id)| *id != param);
        self.outputs.remove(param);
        self.connections.retain(|_, o| *o != param);
    }

    /// Deletes mistyped connection made with param_id
    ///
    /// This is only needed connection param type is changed with means
    /// other than [`Graph::update_input_param`].
    pub fn ensure_connection_types(&mut self, param_id: AnyParameterId) {
        let mut to_remove = Vec::default();

        for (to_id, from_id) in self.iter_connections() {
            // ignore connections that don't touch param_id.
            if AnyParameterId::Input(to_id) != param_id
                && AnyParameterId::Output(from_id) != param_id
            {
                continue;
            }

            // connection has mismatched types
            if self.get_input(to_id).typ != self.get_output(from_id).typ {
                to_remove.push(to_id);
            }
        }

        for in_id in to_remove {
            self.remove_connection(in_id);
        }
    }

    /// Removes a node from the graph with given `node_id`. This also removes
    /// any incoming or outgoing connections from that node
    ///
    /// This function returns the list of connections that has been removed
    /// after deleting this node as input-output pairs. Note that one of the two
    /// ids in the pair (the one on `node_id`'s end) will be invalid after
    /// calling this function.
    pub fn remove_node(&mut self, node_id: NodeId) -> (Node<NodeData>, Vec<(InputId, OutputId)>) {
        let mut disconnect_events = vec![];

        self.connections.retain(|i, o| {
            if self.outputs[*o].node == node_id || self.inputs[i].node == node_id {
                disconnect_events.push((i, *o));
                false
            } else {
                true
            }
        });

        // NOTE: Collect is needed because we can't borrow the input ids while
        // we remove them inside the loop.
        for input in self[node_id].input_ids().collect::<SVec<_>>() {
            self.inputs.remove(input);
        }
        for output in self[node_id].output_ids().collect::<SVec<_>>() {
            self.outputs.remove(output);
        }
        let removed_node = self.nodes.remove(node_id).expect("Node should exist");

        (removed_node, disconnect_events)
    }

    pub fn remove_connection(&mut self, input_id: InputId) -> Option<OutputId> {
        self.connections.remove(input_id)
    }

    pub fn iter_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().map(|(id, _)| id)
    }

    pub fn add_connection(&mut self, output: OutputId, input: InputId) {
        self.connections.insert(input, output);
    }

    pub fn iter_connections(&self) -> impl Iterator<Item = (InputId, OutputId)> + '_ {
        self.connections.iter().map(|(o, i)| (o, *i))
    }

    pub fn connection(&self, input: InputId) -> Option<OutputId> {
        self.connections.get(input).copied()
    }

    pub fn any_param_type(&self, param: AnyParameterId) -> Result<&DataType, EguiGraphError> {
        match param {
            AnyParameterId::Input(input) => self.inputs.get(input).map(|x| &x.typ),
            AnyParameterId::Output(output) => self.outputs.get(output).map(|x| &x.typ),
        }
        .ok_or(EguiGraphError::InvalidParameterId(param))
    }

    pub fn try_get_input(&self, input: InputId) -> Option<&InputParam<DataType, ValueType>> {
        self.inputs.get(input)
    }

    pub fn get_input(&self, input: InputId) -> &InputParam<DataType, ValueType> {
        &self.inputs[input]
    }

    pub fn try_get_output(&self, output: OutputId) -> Option<&OutputParam<DataType>> {
        self.outputs.get(output)
    }

    pub fn get_output(&self, output: OutputId) -> &OutputParam<DataType> {
        &self.outputs[output]
    }
}

impl<NodeData, DataType, ValueType> Default for Graph<NodeData, DataType, ValueType>
where
    DataType: PartialEq,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<NodeData> Node<NodeData> {
    pub fn inputs<'a, DataType: PartialEq, DataValue>(
        &'a self,
        graph: &'a Graph<NodeData, DataType, DataValue>,
    ) -> impl Iterator<Item = &'a InputParam<DataType, DataValue>> + 'a {
        self.input_ids().map(|id| graph.get_input(id))
    }

    pub fn outputs<'a, DataType: PartialEq, DataValue>(
        &'a self,
        graph: &'a Graph<NodeData, DataType, DataValue>,
    ) -> impl Iterator<Item = &'a OutputParam<DataType>> + 'a {
        self.output_ids().map(|id| graph.get_output(id))
    }

    pub fn input_ids(&self) -> impl Iterator<Item = InputId> + '_ {
        self.inputs.iter().map(|(_name, id)| *id)
    }

    pub fn output_ids(&self) -> impl Iterator<Item = OutputId> + '_ {
        self.outputs.iter().map(|(_name, id)| *id)
    }

    pub fn get_input(&self, name: &str) -> Result<InputId, EguiGraphError> {
        self.inputs
            .iter()
            .find(|(param_name, _id)| param_name == name)
            .map(|x| x.1)
            .ok_or_else(|| EguiGraphError::NoParameterNamed(self.id, name.into()))
    }

    pub fn get_output(&self, name: &str) -> Result<OutputId, EguiGraphError> {
        self.outputs
            .iter()
            .find(|(param_name, _id)| param_name == name)
            .map(|x| x.1)
            .ok_or_else(|| EguiGraphError::NoParameterNamed(self.id, name.into()))
    }
}

impl<DataType, ValueType> InputParam<DataType, ValueType> {
    pub fn value(&self) -> &ValueType {
        &self.value
    }

    pub fn kind(&self) -> InputParamKind {
        self.kind
    }

    pub fn node(&self) -> NodeId {
        self.node
    }
}
