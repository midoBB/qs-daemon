#!/bin/bash

# SPDX-License-Identifier: MPL-2.0

# Quickfile daemon client script  
# Usage: quickfile-client search "query"
#        quickfile-client refresh
#        quickfile-client status

REQUEST_SOCKET="/tmp/quickfile-daemon.sock"
RESPONSE_SOCKET="/tmp/quickfile-response.sock" 
RESPONSE_TIMEOUT=5

# Check if request socket exists
if [ ! -S "$REQUEST_SOCKET" ]; then
    echo '{"type":"Error","message":"Daemon not running"}' >&2
    exit 1
fi

# Function to send request and get response using 2-socket method
send_request_2socket() {
    local request="$1"
    local response_file=$(mktemp)
    local response_pid
    
    # Try to listen on response socket in background
    if [ -S "$RESPONSE_SOCKET" ]; then
        socat UNIX-CONNECT:$RESPONSE_SOCKET STDOUT > "$response_file" &
        response_pid=$!
        
        # Give a moment for response listener to connect
        sleep 0.1
        
        # Send request
        echo "$request" | socat - UNIX-CONNECT:$REQUEST_SOCKET
        
        # Wait for response with timeout
        local count=0
        while [ $count -lt $((RESPONSE_TIMEOUT * 10)) ] && kill -0 $response_pid 2>/dev/null; do
            if [ -s "$response_file" ]; then
                cat "$response_file"
                kill $response_pid 2>/dev/null
                rm -f "$response_file"
                return 0
            fi
            sleep 0.1
            count=$((count + 1))
        done
        
        # Cleanup if timeout or error
        kill $response_pid 2>/dev/null
        rm -f "$response_file"
    fi
    
    # Fallback to single socket method
    echo "$request" | socat - UNIX-CONNECT:$REQUEST_SOCKET
    rm -f "$response_file"
}

# Build request based on command
case "$1" in
    "search")
        QUERY="${2:-}"
        REQUEST="{\"type\":\"Search\",\"query\":\"$QUERY\",\"limit\":100}"
        ;;
    "refresh")
        REQUEST="{\"type\":\"Refresh\"}"
        ;;
    "status")
        REQUEST="{\"type\":\"Status\"}"
        ;;
    *)
        echo "Usage: $0 {search <query>|refresh|status}" >&2
        exit 1
        ;;
esac

# Send request and display response
send_request_2socket "$REQUEST"
