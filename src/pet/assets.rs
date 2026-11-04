pub struct AssetStore {
    model: Option<Vec<u8>>,
    config: Vec<u8>,
}

impl AssetStore {
    pub fn new(model: Option<Vec<u8>>, config: Vec<u8>) -> Self {
        Self { model, config }
    }

    pub fn get(&self, path: &str) -> Option<(&str, &[u8])> {
        match path {
            "/pet.glb" if self.model.is_some() => {
                Some(("model/gltf-binary", self.model.as_deref()?))
            }
            "/config.json" => Some(("application/json", &self.config)),
            "/" => Some(("text/html", include_bytes!("../../assets/pet/index.html"))),
            "/pet.css" => Some(("text/css", include_bytes!("../../assets/pet/pet.css"))),
            "/pet.js" => Some(("text/javascript", include_bytes!("../../assets/pet/pet.js"))),
            "/toon.js" => Some((
                "text/javascript",
                include_bytes!("../../assets/pet/toon.js"),
            )),
            "/vendor/three.module.js" => Some((
                "text/javascript",
                include_bytes!("../../assets/pet/vendor/three.module.js"),
            )),
            "/vendor/GLTFLoader.js" => Some((
                "text/javascript",
                include_bytes!("../../assets/pet/vendor/GLTFLoader.js"),
            )),
            "/vendor/BufferGeometryUtils.js" => Some((
                "text/javascript",
                include_bytes!("../../assets/pet/vendor/BufferGeometryUtils.js"),
            )),
            "/pet.glb" => Some(("model/gltf-binary", include_bytes!("../../assets/pet.glb"))),
            _ => None,
        }
    }
}
