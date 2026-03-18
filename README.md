# rmock

This project is a dependency-free and multiplatform mock server. 
Download the executable, create the routes.yml file and execute it.

## Installation and usage

Go to the [releases](https://github.com/joseph-onigiri/mock-server/releases) page and download the latest release for your platform.

Once you have the executable, create a `routes.yml` with the routes you want to mock. You can find an example in the `examples` folder.


### `routes.yml` format

The `routes.yml` file is a yaml file with the following format (the `[]` params are optional):

```
routes.yml
├── [default] // Optional global configuration
│   ├── [proxy]: string // If defined, any missing route will go through this proxy
│   └── [port]: number // If defined, the server will use this value
└── endpoints // The collection of the endpoints
    ├── path: string // Server endpoint path
    ├── method: string // One of the http available methods
    └── response // Object containing the response
        ├── [status]: number // The response status code
        ├── [headers]: [string, string] // A hashmap with the response headers
        └── [body]: string // The string response body
```

### Program usage

You can run the server with the following command:

```shell
./mock-server
```

If you are not using the default `routes.yml` file, you can pass the path to the file as an argument:

```shell
./mock-server ./files/configuration_1.yml
```

You can also pass the port as an argument:

```shell
./mock-server --port 8080
```

This prevalece over the `port` value in the `routes.yml` file.