class Mycelia < Formula
  desc "Local, content-agnostic knowledge index for agent retrieval"
  homepage "https://github.com/dnlbox/mycelia"
  url "https://github.com/dnlbox/mycelia.git", tag: "v0.1.0"
  license "Apache-2.0"
  head "https://github.com/dnlbox/mycelia.git", branch: "main"

  depends_on "rust" => :build
  depends_on "onnxruntime"

  def install
    system "cargo", "install",
      *std_cargo_args(path: "crates/mycelia-cli"),
      "--no-default-features",
      "--features", "semantic-system-ort"

    real_binary = libexec/"mycelia"
    libexec.install bin/"mycelia" => real_binary.basename

    onnxruntime = formula_opt_lib("onnxruntime").shared_library("libonnxruntime")
    (bin/"mycelia").write <<~SH
      #!/bin/sh
      export ORT_DYLIB_PATH="${ORT_DYLIB_PATH:-#{onnxruntime}}"
      exec "#{real_binary}" "$@"
    SH
    chmod 0755, bin/"mycelia"
  end

  test do
    (testpath/"corpus").mkpath
    (testpath/"corpus/notes.txt").write("alpha beta answer\n")

    system bin/"mycelia", "index",
      testpath/"corpus",
      "--database",
      testpath/"index.sqlite3"

    output = shell_output("#{bin}/mycelia find 'alpha answer' " \
                          "--database #{testpath}/index.sqlite3 --strategy fts5-reranked")

    assert_match "notes.txt", output
  end
end
