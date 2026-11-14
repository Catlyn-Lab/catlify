import Foundation

/// Thin Swift wrapper over the Rust JSON RPC (see src-tauri/src/rpc.rs).
enum StafyEngine {
    @discardableResult
    static func call(_ method: String, _ args: [String: Any] = [:]) -> Any? {
        let data = (try? JSONSerialization.data(withJSONObject: args)) ?? Data("{}".utf8)
        let argsJson = String(data: data, encoding: .utf8) ?? "{}"
        guard let raw = stafy_call(method, argsJson) else { return nil }
        defer { stafy_free(raw) }
        guard let obj = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8)) as? [String: Any] else { return nil }
        if let error = obj["error"] as? String {
            NSLog("[Catlify] %@ failed: %@", method, error)
            return nil
        }
        return obj["ok"]
    }

    static func control(_ action: String) { call("control", ["action": action]) }

    struct Node {
        let id: String
        let title: String
        let subtitle: String?
        let art: String?
        let playable: Bool
        let browsable: Bool
        /// Section heading ("Charts", "Genres", "Moods"…), if the list has sections.
        let group: String?
    }

    static func children(_ parent: String) -> [Node] {
        let list = call("children", ["parent": parent]) as? [[String: Any]] ?? []
        return list.map {
            Node(id: $0["id"] as? String ?? "", title: $0["title"] as? String ?? "",
                 subtitle: $0["subtitle"] as? String, art: $0["art"] as? String,
                 playable: $0["playable"] as? Bool ?? false, browsable: $0["browsable"] as? Bool ?? false,
                 group: $0["group"] as? String)
        }
    }

    static var carAutoplay: Bool {
        ((call("settings") as? [String: Any])?["car_autoplay"] as? Bool) ?? true
    }
}
