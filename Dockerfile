FROM rust:1.45 as builder

ENV APP_HOME /home/app
RUN mkdir -p $APP_HOME
WORKDIR $APP_HOME

# build essentials
RUN apt-get update -y && \
    apt-get install -y clang cmake

# codec and nanomsg (with workaround for proper link to libnanomsg.so)
RUN apt install -y libx264-dev libvpx-dev libopus-dev nanomsg-utils && \
    ln -s libnanomsg.so.5 /usr/lib/x86_64-linux-gnu/libnanomsg.so

# mame link dependency
RUN apt install -y libsdl2-dev qt5-default

# gipan
COPY Cargo.lock Cargo.toml $APP_HOME/
ADD src $APP_HOME/src
ADD libemu $APP_HOME/libemu
ADD libenc $APP_HOME/libenc

# ADD mame $APP_HOME/mame
RUN wget https://oraksil.s3.ap-northeast-2.amazonaws.com/etc/libmame64.so -P $APP_HOME/mame

RUN cargo build --release


FROM debian:buster

RUN apt-get update -y && \
    apt-get install -y libx264-dev libvpx-dev libopus-dev nanomsg-utils && \
    ln -s libnanomsg.so.5 /usr/lib/x86_64-linux-gnu/libnanomsg.so && \
	apt-get install -y libsdl2-dev qt5-default

ENV APP_HOME /home/app
WORKDIR $APP_HOME

COPY --from=builder $APP_HOME/target/release/gipan $APP_HOME/
COPY --from=builder $APP_HOME/mame/libmame64.so $APP_HOME/mame/
COPY docker-entry.sh $APP_HOME/
ADD bgfx $APP_HOME/bgfx
ADD cfg $APP_HOME/cfg
ADD nvram $APP_HOME/nvram
ADD roms $APP_HOME/roms

CMD ["./docker-entry.sh"]
