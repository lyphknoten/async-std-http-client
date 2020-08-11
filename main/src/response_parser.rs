use crate::model::*;
use async_std::prelude::*;
use http::{
    header::{HeaderName, CONNECTION /*, CONTENT_LENGTH*/},
    status::StatusCode,
    HeaderMap, HeaderValue,
};
use http_parser::*;

pub async fn read_reponse(
    rw: &mut Connection,
    _config: &ClientConfig,
) -> Result<(Response, ConnectionState)> {
    log::debug!("starting to read response");

    let mut parser = HttpParser::new(HttpParserType::Response);

    let mut cb = Callback {
        error: None,
        body: Vec::new(),
        headers: HeaderMap::with_capacity(10),
        last_header_name: None,
        status: None,
        is_message_complete: false,
    };

    let mut buffer: [u8; 4096] = [0; 4096];

    loop {
        let bytes_read = rw.1.read(&mut buffer).await.map_err(|err| Error {
            text: format!("lost connection while reading: {}", err),
        })?;

        if bytes_read == 0 {
            // TODO: is this fine if we are in http 1.0-mode`?

            return Err(Error {
                text: format!("unepected EOF"),
            });
        }

        parser.execute(&mut cb, &buffer[..bytes_read]);

        if let Some(err) = cb.error {
            return Err(err);
        }

        if cb.is_message_complete {
            let response = cb.to_response()?;

            let connection_state = if response
                .headers
                .get(CONNECTION)
                .iter()
                .flat_map(|c| c.to_str())
                .any(|c| c == "keep-alive")
            {
                ConnectionState::KeepAlive
            } else {
                ConnectionState::Close
            };

            // let content_length: Option<usize> = response
            //     .headers
            //     .get(CONTENT_LENGTH)
            //     .iter()
            //     .flat_map(|c| c.to_str())
            //     .flat_map(|s| s.parse())
            //     .next();

            // dbg!(content_length);

            log::debug!(
                "successfull readed reponse, status {}, {} bytes, {} headers, connection: {:?}",
                response.status,
                response.body.len(),
                response.headers.len(),
                connection_state
            );

            return Ok((response, ConnectionState::KeepAlive));
        }
    }
}

struct Callback {
    error: Option<Error>,
    body: Vec<u8>,
    headers: HeaderMap,
    last_header_name: Option<HeaderName>,
    status: Option<StatusCode>,
    is_message_complete: bool,
}

impl Callback {
    fn to_response(mut self) -> Result<Response> {
        match self.status {
            Some(status) => {
                let mut response = Response {
                    status,
                    headers: HeaderMap::with_capacity(0),
                    body: Vec::with_capacity(0),
                };

                std::mem::swap(&mut response.headers, &mut self.headers);

                if self.body.len() > 0 {
                    std::mem::swap(&mut response.body, &mut self.body);
                }

                Ok(response)
            }
            None => Err(Error {
                text: format!("illformed header"),
            }),
        }
    }
}

impl HttpParserCallback for Callback {
    fn on_message_begin(&mut self, _parser: &mut HttpParser) -> CallbackResult {
        Ok(http_parser::ParseAction::None)
    }

    fn on_status(&mut self, parser: &mut HttpParser, _data: &[u8]) -> CallbackResult {
        match parser
            .status_code
            .and_then(|s| StatusCode::from_u16(s).ok())
        {
            Some(status) => self.status = Some(status),
            None => {
                self.error = Some(Error {
                    text: format!("illformed reponse"),
                })
            }
        }

        Ok(http_parser::ParseAction::None)
    }

    fn on_header_field(&mut self, _parser: &mut HttpParser, data: &[u8]) -> CallbackResult {
        match HeaderName::from_bytes(data) {
            Ok(n) => self.last_header_name = Some(n),
            Err(_) => {
                self.error = Some(Error {
                    text: format!("illformed header"),
                })
            }
        }
        Ok(ParseAction::None)
    }

    fn on_header_value(&mut self, _parser: &mut HttpParser, data: &[u8]) -> CallbackResult {
        match (
            // TODO: could we take ownership here?
            self.last_header_name.clone(),
            String::from_utf8(data.to_vec())
                .map_err(|_| ())
                .and_then(|v| v.parse::<HeaderValue>().map_err(|_| ())),
        ) {
            (Some(n), Ok(v)) => {
                self.headers.insert(n, v);
                self.last_header_name = None;
            }
            _ => {
                self.error = Some(Error {
                    text: format!("illformed header"),
                })
            }
        }
        Ok(ParseAction::None)
    }

    fn on_headers_complete(&mut self, _parser: &mut HttpParser) -> CallbackResult {
        Ok(ParseAction::None)
    }

    fn on_body(&mut self, _parser: &mut HttpParser, data: &[u8]) -> CallbackResult {
        self.body.extend(data);
        Ok(ParseAction::None)
    }

    fn on_message_complete(&mut self, _parser: &mut HttpParser) -> CallbackResult {
        self.is_message_complete = true;
        Ok(ParseAction::None)
    }
}