fn main() {
    for file in ["logo", "logo_text"] {
        let mut buf: Vec<u8> = Vec::new();

        println!("cargo:rerun-if-changed=./{file}.png");
        let img = image::open(format!("./{file}.png")).unwrap();
        let image::DynamicImage::ImageRgba8(img) = img else { panic!() };

        buf.extend(img.width().to_be_bytes());
        buf.extend(img.height().to_be_bytes());

        for pixel in img.pixels() {
            buf.extend(pixel.0);
        }

        std::fs::write(format!("./{file}.rgba"), buf).unwrap();
    }
}
