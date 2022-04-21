---
title: "Configure Snowplow"
description: "How to configure Snowplow to connect to Materialize"
menu:
  main:
    parent: "integration-guides"
    name: "Snowplow"
    weight: 10
---

[Snowplow](https://snowplowanalytics.com/) is an open source data collection platform.

The standard Snowplow setup involves installing a [Snowplow tracker](https://docs.snowplowanalytics.com/docs/collecting-data/collecting-from-own-applications/) on your website that begins emitting events to a [Snowplow collector](https://docs.snowplowanalytics.com/docs/pipeline-components-and-applications/stream-collector/) installed on AWS or GCP. This collector serializes the raw events to a Thrift record format, which are then processed by two downstream consumers:

* a enriched version of the events that gets written to another stream, i.e. Kafka or Kinesis
* a compressed version that gets written to S3

Once the events are in Kafka, Kinesis, or S3, you can connect Materialize to obstain real-time views over the streams.

<!-- Website -- Snowplow Tracker -- Snowplow Collector -- Kinesis/Kafka -- S3 -- Materialize. -->

This guide assumes you are using the open source version of Snowplow, Snowplow Open Source and not the managed service, [Snowplow BDP](https://docs.snowplowanalytics.com/docs/getting-started-with-snowplow-bdp/what-is-snowplow-bdp/).

## Snowplow + Kinesis + S3

1. Follow all the steps in the [Snowplow Open Source Quickstart](https://docs.snowplowanalytics.com/docs/open-source-quick-start/quick-start-installation-guide-on-aws/)

    This guide walks you through the default way of setting up Snowplow on AWS, which is with Kinesis + S3 as the data sinks. In particular, Snowplow S3 Loader [consumes records from an Amazon Kinesis stream](https://docs.snowplowanalytics.com/docs/pipeline-components-and-applications/loaders-storage-targets/s3-loader/) and writes them to S3.

    After following all the steps in this guide, you should have an S3 bucket and several Kinesis streams in your AWS console.

    ![Snowplow kinesis](/images/integrations/snowplow-kinesis.png)

    ![Snowplow s3](/images/integrations/snowplow-s3.png)

1. Connect Materialize to Kinesis

    After following the Snowplow Open Source Quickstart, you should have your AWS CLI configured.

    a. Follow the instructions [here](https://docs.aws.amazon.com/cli/latest/userguide/cli-services-iam-create-creds.html) to generate an access key using the AWS CLI
    
    b. Run the following command to find the ARN of the kinesis stream you set up in Step 1:

    ```bash
    aws kinesis describe-stream --stream-name good_sink
    ```

    c. Connect to your Materialize instance using any Materialize-compatible CLI, like `psql`.

    ```bash
    psql -U materialize -h localhost -p 6875 materialize
    ```

    d. From psql, run one of [these](https://materialize.com/docs/sql/create-source/kinesis/) `CREATE SOURCE` commands with the `arn:aws:kinesis:aws-region::stream/fake-stream`, `access_key_id`, and `secret_access_key` values replaced.

    ```psql
    CREATE SOURCE json_source
      FROM KINESIS ARN 'ARN'
      WITH ( access_key_id = 'access_key_id',
            secret_access_key = 'secret_access_key' )
      FORMAT BYTES;
    ```

    e. You can also connect S3 to Materialize by following the instructions [here](arn:aws:kinesis:aws-region::stream/fake-stream).


## Snowplow + Kafka

1. Create a Kafka cluster and topic in Upstash

    a. Create or sign in to your account on Upstash and select **Kafka** from the nav bar at the top of the page

    b. Click **Create Cluster**

    c. Click **Create Topic** and name it `good_sink`

    d. Click **Create Topic** again and name it `bad_sink`

    e. On the **Details** tab of the cluster, you'll see the connection details for the cluster at the top:

        Region
        Endpoint
        Username
        Password

      Note these connection details - you will need them later.

1. Install the Snowplow Scala Kafka Collector

    The Snowplow collector receives events sent to it from event tackers that instrument your website. There are [different flavors of Snowplow collectors]([Snowplow Open Source Quickstart](https://docs.snowplowanalytics.com/docs/open-source-quick-start/quick-start-installation-guide-on-aws/)) depending on which data sink you are using. Here we are using a Kafka sink. 

    You will likely be deploying your Snowplow Collector on AWS or GCP instance - if so, follow these additional steps to set up Docker on AWS or [GCP](https://cloud.google.com/compute/docs/containers).

    a. Pull down the docker image for the Kafka flavor of the stream collector

    ```bash
    docker pull snowplow/scala-stream-collector-kafka:2.4.5
    ```

    b. Create a `config.hocon` file in same `pwd` location that you will be running your docker commands from

    Copy and paste the following config into the file with the `upstash_endpoint`, `upstash_username`, and `upstash_password` replaced with the Upstash Kafka cluster connection details you collected above.

    ```hocon
    collector {
      interface = "0.0.0.0"
      interface = ${?COLLECTOR_INTERFACE}
      port = 8080
      port = ${?COLLECTOR_PORT}

      ssl {
        enable = false
        enable = ${?COLLECTOR_SSL}
        # whether to redirect HTTP to HTTPS
        redirect = false
        redirect = ${?COLLECTOR_SSL_REDIRECT}
        port = 9543
        port = ${?COLLECTOR_SSL_PORT}
      }

      paths {
        # "/com.acme/track" = "/com.snowplowanalytics.snowplow/tp2"
        # "/com.acme/redirect" = "/r/tp2"
        # "/com.acme/iglu" = "/com.snowplowanalytics.iglu/v1"
      }

      p3p {
        policyRef = "/w3c/p3p.xml"
        CP = "NOI DSP COR NID PSA OUR IND COM NAV STA"
      }

      crossDomain {
        enabled = false
        # Domains that are granted access, *.acme.com will match http://acme.com and http://sub.acme.com
        enabled = ${?COLLECTOR_CROSS_DOMAIN_ENABLED}
        domains = [ "*" ]
        domains = [ ${?COLLECTOR_CROSS_DOMAIN_DOMAIN} ]
        # Whether to only grant access to HTTPS or both HTTPS and HTTP sources
        secure = true
        secure = ${?COLLECTOR_CROSS_DOMAIN_SECURE}
      }

      cookie {
        enabled = true
        #enabled = ${?COLLECTOR_COOKIE_ENABLED}
        expiration = "365 days" # e.g. "365 days"
        #expiration = ${?COLLECTOR_COOKIE_EXPIRATION}
        # Network cookie name
        name = charg

        secure = false
        secure = ${?COLLECTOR_COOKIE_SECURE}
        httpOnly = false
        httpOnly = ${?COLLECTOR_COOKIE_HTTP_ONLY}

      }


      doNotTrackCookie {
        enabled = false
        enabled = ${?COLLECTOR_DO_NOT_TRACK_COOKIE_ENABLED}
        name = ""
        name = ${?COLLECTOR_DO_NOT_TRACK_COOKIE_NAME}
        value = ""
        value = ${?COLLECTOR_DO_NOT_TRACK_COOKIE_VALUE}
      }

      cookieBounce {
        enabled = false
        enabled = ${?COLLECTOR_COOKIE_BOUNCE_ENABLED}
        # The name of the request parameter which will be used on redirects checking that third-party
        # cookies work.
        name = "n3pc"
        name = ${?COLLECTOR_COOKIE_BOUNCE_NAME}
        # Network user id to fallback to when third-party cookies are blocked.
        fallbackNetworkUserId = "00000000-0000-4000-A000-000000000000"
        fallbackNetworkUserId = ${?COLLECTOR_COOKIE_BOUNCE_FALLBACK_NETWORK_USER_ID}
        # Optionally, specify the name of the header containing the originating protocol for use in the
        # bounce redirect location. Use this if behind a load balancer that performs SSL termination.
        # The value of this header must be http or https. Example, if behind an AWS Classic ELB.
        forwardedProtocolHeader = "X-Forwarded-Proto"
        forwardedProtocolHeader = ${?COLLECTOR_COOKIE_BOUNCE_FORWARDED_PROTOCOL_HEADER}
      }

      enableDefaultRedirect = true
      enableDefaultRedirect = ${?COLLECTOR_ALLOW_REDIRECTS}

      redirectMacro {
        enabled = false
        enabled = ${?COLLECTOR_REDIRECT_MACRO_ENABLED}
        # Optional custom placeholder token (defaults to the literal `${SP_NUID}`)
        placeholder = "[TOKEN]"
        placeholder = ${?COLLECTOR_REDIRECT_REDIRECT_MACRO_PLACEHOLDER}
      }

      rootResponse {
        enabled = false
        enabled = ${?COLLECTOR_ROOT_RESPONSE_ENABLED}
        statusCode = 302
        statusCode = ${?COLLECTOR_ROOT_RESPONSE_STATUS_CODE}
        # Optional, defaults to empty map
        headers = {
          Location = "https://127.0.0.1/",
          Location = ${?COLLECTOR_ROOT_RESPONSE_HEADERS_LOCATION},
          X-Custom = "something"
        }
        # Optional, defaults to empty string
        body = "302, redirecting"
        body = ${?COLLECTOR_ROOT_RESPONSE_BODY}
      }

      # Configuration related to CORS preflight requests
      cors {
        # The Access-Control-Max-Age response header indicates how long the results of a preflight
        # request can be cached. -1 seconds disables the cache. Chromium max is 10m, Firefox is 24h.
        accessControlMaxAge = 5 seconds
        accessControlMaxAge = ${?COLLECTOR_CORS_ACCESS_CONTROL_MAX_AGE}
      }

      # Configuration of prometheus http metrics
      prometheusMetrics {
        # If metrics are enabled then all requests will be logged as prometheus metrics
        # and '/metrics' endpoint will return the report about the requests
        enabled = false
        # Custom buckets for http_request_duration_seconds_bucket duration metric
        #durationBucketsInSeconds = [0.1, 3, 10]
      }

      streams {
        # Events which have successfully been collected will be stored in the good stream/topic
        good = good_sink		
        good = ${?COLLECTOR_STREAMS_GOOD}

        # Events that are too big (w.r.t Kinesis 1MB limit) will be stored in the bad stream/topic
        bad = bad_sink
        bad = ${?COLLECTOR_STREAMS_BAD}

        # Whether to use the incoming events ip as the partition key for the good stream/topic
        # Note: Nsq does not make use of partition key.
        useIpAddressAsPartitionKey = false
        useIpAddressAsPartitionKey = ${?COLLECTOR_STREAMS_USE_IP_ADDRESS_AS_PARTITION_KEY}

        # Enable the chosen sink by uncommenting the appropriate configuration
        sink {
          # Choose between kinesis, google-pub-sub, kafka, nsq, or stdout.
          # To use stdout, comment or remove everything in the "collector.streams.sink" section except
          # "enabled" which should be set to "stdout".
          enabled = kafka
      
          # replace with your Upstash Kafka broker 
          brokers = "upstash_endpoint"

          ## Number of retries to perform before giving up on sending a record
          retries = 0
          # The kafka producer has a variety of possible configuration options defined at
          # https://kafka.apache.org/documentation/#producerconfigs
          # Some values are set to other values from this config by default:
          #"bootstrap.servers" -> brokers
          #retries             -> retries
          #"buffer.memory"     -> buffer.byteLimit
          #"linger.ms"         -> buffer.timeLimit
          producerConf {
            "sasl.jaas.config" = "org.apache.kafka.common.security.scram.ScramLoginModule required username='upstash_username' password='upstash_password"
            "security.protocol" = "SASL_SSL"
            "sasl.mechanism" = "SCRAM-SHA-256"
          }


        }

        buffer {
          byteLimit = 4500000
          byteLimit = ${?COLLECTOR_STREAMS_BUFFER_BYTE_LIMIT}
          recordLimit = 500  # Not supported by Kafka; will be ignored
          recordLimit = ${?COLLECTOR_STREAMS_BUFFER_RECORD_LIMIT}
          timeLimit = 60000
          timeLimit = ${?COLLECTOR_STREAMS_BUFFER_TIME_LIMIT}
        }
      }

    }

    akka {
      loglevel = DEBUG # 'OFF' for no logging, 'DEBUG' for all logging.
      loglevel = ${?AKKA_LOGLEVEL}
      loggers = ["akka.event.slf4j.Slf4jLogger"]
      loggers = [${?AKKA_LOGGERS}]

      http.server {
        # To obtain the hostname in the collector, the 'remote-address' header
        # should be set. By default, this is disabled, and enabling it
        # adds the 'Remote-Address' header to every request automatically.
        remote-address-header = on
        remote-address-header = ${?AKKA_HTTP_SERVER_REMOTE_ADDRESS_HEADER}

        raw-request-uri-header = on
        raw-request-uri-header = ${?AKKA_HTTP_SERVER_RAW_REQUEST_URI_HEADER}

        # Define the maximum request length (the default is 2048)
        parsing {
          max-uri-length = 32768
          max-uri-length = ${?AKKA_HTTP_SERVER_PARSING_MAX_URI_LENGTH}
          uri-parsing-mode = relaxed
          uri-parsing-mode = ${?AKKA_HTTP_SERVER_PARSING_URI_PARSING_MODE}
        }
      }

    }
    ```
    Now run the Snowplow collector application, passing in the `config.hocon` file you just created.

    ```bash
    docker run --rm \
    -v $PWD/config.hocon:/snowplow/config.hocon \
    -p 8080:8080 \
    snowplow/scala-stream-collector-kafka:2.4.5 --config /snowplow/config.hocon
    ```
    If everything is successful, you should see something like the message below in stdout:

    ```console
    [DEBUG] [04/21/2022 20:05:11.781] [scala-stream-collector-akka.actor.internal-dispatcher-4] [akka://scala-stream-collector/system/IO-TCP/selectors/$a/0] Successfully bound to /0.0.0.0:8080
    [scala-stream-collector-akka.actor.default-dispatcher-5] INFO com.snowplowanalytics.snowplow.collectors.scalastream.KafkaCollector$ - REST interface bound to /0.0.0.0:8080
    ```

3. Start emitting events to the Snowplow Kafka Collector

    Snowplow provides [trackers](https://docs.snowplowanalytics.com/docs/collecting-data/) in different languages that you can embed into your website and send events to the Collector. Refer to the Snowplow documentation for more instructions on how you can get that set up.
    
    Just so we can test our setup here, let's send a simple request using cURL from your terminal. This example is a typical page_view event, which has been taken from the docs.snowplowanalytics.com website.

    The example will also send a sample “failed event” (a custom product_view event that will fail due to an appropriate schema not being available to validate against) so that you can get a better understanding of how bad events are generated and what they look like.

    ```bash
    curl 'http://localhost:8080/com.snowplowanalytics.snowplow/tp2' \
    -H 'Content-Type: application/json; charset=UTF-8' \
    -H 'Cookie: _sp=305902ac-8d59-479c-ad4c-82d4a2e6bb9c' \
    --data-raw '{"schema":"iglu:com.snowplowanalytics.snowplow/payload_data/jsonschema/1-0-4","data":[{"e":"pv","url":"https://docs.snowplowanalytics.com/docs/send-test-events-to-your-pipeline/","page":"Send test events to your pipeline - Snowplow Docs","refr":"https://docs.snowplowanalytics.com/","tv":"js-2.17.2","tna":"spExample","aid":"docs-example","p":"web","tz":"Europe/London","lang":"en-GB","cs":"UTF-8","res":"3440x1440","cd":"24","cookie":"1","eid":"4e35e8c6-03c4-4c17-8202-80de5bd9d953","dtm":"1626182778191","cx":"eyJzY2hlbWEiOiJpZ2x1OmNvbS5zbm93cGxvd2FuYWx5dGljcy5zbm93cGxvdy9jb250ZXh0cy9qc29uc2NoZW1hLzEtMC0wIiwiZGF0YSI6W3sic2NoZW1hIjoiaWdsdTpjb20uc25vd3Bsb3dhbmFseXRpY3Muc25vd3Bsb3cvd2ViX3BhZ2UvanNvbnNjaGVtYS8xLTAtMCIsImRhdGEiOnsiaWQiOiI0YTU2ZjQyNy05MTk2LTQyZDEtOWE0YS03ZjRlNzk2OTM3ZmEifX1dfQ","vp":"863x1299","ds":"848x5315","vid":"3","sid":"87c18fc8-2055-4ec4-8ad6-fff64081c2f3","duid":"5f06dbb0-a893-472b-b61a-7844032ab3d6","stm":"1626182778194"},{"e":"ue","ue_px":"eyJzY2hlbWEiOiJpZ2x1OmNvbS5zbm93cGxvd2FuYWx5dGljcy5zbm93cGxvdy91bnN0cnVjdF9ldmVudC9qc29uc2NoZW1hLzEtMC0wIiwiZGF0YSI6eyJzY2hlbWEiOiJpZ2x1OmNvbS5teV9jb21wYW55L3Byb2R1Y3Rfdmlldy9qc29uc2NoZW1hLzEtMC0wIiwiZGF0YSI6eyJpZCI6IjVOMFctUEwwVyIsImN1cnJlbnRfcHJpY2UiOjQ0Ljk5LCJkZXNjcmlwdGlvbiI6IlB1cnBsZSBTbm93cGxvdyBIb29kaWUifX19","tv":"js-2.17.2","tna":"spExample","aid":"docs-example","p":"web","tz":"Europe/London","lang":"en-GB","cs":"UTF-8","res":"3440x1440","cd":"24","cookie":"1","eid":"542a79d3-a3b8-421c-99d6-543ff140a56a","dtm":"1626182778193","cx":"eyJzY2hlbWEiOiJpZ2x1OmNvbS5zbm93cGxvd2FuYWx5dGljcy5zbm93cGxvdy9jb250ZXh0cy9qc29uc2NoZW1hLzEtMC0wIiwiZGF0YSI6W3sic2NoZW1hIjoiaWdsdTpjb20uc25vd3Bsb3dhbmFseXRpY3Muc25vd3Bsb3cvd2ViX3BhZ2UvanNvbnNjaGVtYS8xLTAtMCIsImRhdGEiOnsiaWQiOiI0YTU2ZjQyNy05MTk2LTQyZDEtOWE0YS03ZjRlNzk2OTM3ZmEifX1dfQ","vp":"863x1299","ds":"848x5315","vid":"3","sid":"87c18fc8-2055-4ec4-8ad6-fff64081c2f3","duid":"5f06dbb0-a893-472b-b61a-7844032ab3d6","refr":"https://docs.snowplowanalytics.com/","url":"https://docs.snowplowanalytics.com/docs/send-test-events-to-your-pipeline/","stm":"1626182778194"}]}'
    ```

    You should receive an `ok` upon running this curl command; in the console where the collector is running, you should see the following output:

    ```console
    [DEBUG] [04/21/2022 20:11:28.347] [scala-stream-collector-akka.actor.internal-dispatcher-2] [akka://scala-stream-collector/system/IO-TCP/selectors/$a/0] New connection accepted
    ```

    Finally, go into your Upstash Kafka Cluster and under the **Usage** tab check that the event has actually shown up in your Kafka cluster.

4. Connect Materialize to your Upstash Kafka Cluster

    a. Gather the connection details to your Upstash Kafka cluster that you copied down in step 1

    b. Connect to your Materialize instance using any Materialize-compatible CLI, like `psql`.

    ```bash
    psql -U materialize -h localhost -p 6875 materialize
    ```

    c. Replace `localhost:9092` with with the Upstash Kafka cluster endpoint that you copied down in step 1 and run [one of these](https://materialize.com/docs/sql/create-source/kafka/#creating-a-source) `CREATE SOURCE` commands.
