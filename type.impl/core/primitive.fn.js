(function() {
    var type_impls = Object.fromEntries([["hermit_entry",[]],["smoltcp",[]],["x86_64",[]]]);
    if (window.register_type_impls) {
        window.register_type_impls(type_impls);
    } else {
        window.pending_type_impls = type_impls;
    }
})()
//{"start":55,"fragment_lengths":[19,15,14]}