fn main() {
    let mut config = prost_build::Config::new();
    config.out_dir("src");
    
    // Build auth service
    config
        .compile_protos(&["auth.proto"], &["."])
        .unwrap();
    
    // Build activity service
    config
        .compile_protos(&["activity.proto"], &["."])
        .unwrap();
    
    // Build screenshot service
    config
        .compile_protos(&["screenshot.proto"], &["."])
        .unwrap();
}
