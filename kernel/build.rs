fn main() {
    println!("cargo:rustc-link-arg=-T./kernel/linker.ld");
    println!("cargo:rustc-link-arg=--no-dynamic-linker");
    println!("cargo:rerun-if-changed=linker.ld");

    let out_dir = std::env::var("OUT_DIR").unwrap();

    for file in ["logo", "logo_text"] {
        println!("cargo:rerun-if-changed=./assets/{file}.png");
        let img = image::open(format!("./assets/{file}.png")).unwrap();
        let image::DynamicImage::ImageRgba8(img) = img else {
            panic!()
        };

        let mut buf: Vec<u8> = Vec::new();

        buf.extend(img.width().to_le_bytes());
        buf.extend(img.height().to_le_bytes());

        let mut last = [0; 4];
        let mut count: u16 = 0;
        for pixel in img.pixels() {
            if pixel.0 == last {
                count += 1;
            } else {
                if count > 0 {
                    buf.extend(count.to_le_bytes());
                    buf.extend(last);
                }
                last = pixel.0;
                count = 1;
            }
        }

        std::fs::write(format!("{out_dir}/{file}.rgba"), buf).unwrap();
    }
}
