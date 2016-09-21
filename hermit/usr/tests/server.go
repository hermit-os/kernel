// Copyright © 2016 Alan A. A. Donovan & Brian W. Kernighan.
// License: https://creativecommons.org/licenses/by-nc-sa/4.0/

// The original code was published at http://www.gopl.io, see page 21.

// This is an "echo" server that displays request parameters.

package main

import (
	"fmt"
	"log"
	"net/http"
)

func main() {
	fmt.Println("This is an \"echo\" server that displays request parameters.")
	fmt.Println("Start the server and send a http request to it (e.g.")
	fmt.Println("curl http://localhost:8000/help). The server uses port 8000.")
	fmt.Println("If KVM is implicitly started by our proxy, please open the port by")
	fmt.Println("setting the environment variable HERMIT_APP_PORT to 8000.")

	http.HandleFunc("/", handler)
	log.Fatal(http.ListenAndServe(":8000", nil))
}

//!+handler
// handler echoes the HTTP request.
func handler(w http.ResponseWriter, r *http.Request) {
	fmt.Fprintf(w, "%s %s %s\n", r.Method, r.URL, r.Proto)
	for k, v := range r.Header {
		fmt.Fprintf(w, "Header[%q] = %q\n", k, v)
	}
	fmt.Fprintf(w, "Host = %q\n", r.Host)
	fmt.Fprintf(w, "RemoteAddr = %q\n", r.RemoteAddr)
	if err := r.ParseForm(); err != nil {
		log.Print(err)
	}
	for k, v := range r.Form {
		fmt.Fprintf(w, "Form[%q] = %q\n", k, v)
	}
}
//!-handler
